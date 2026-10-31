"""Process-ownership helpers for C.U.R.E native test harnesses.

Every harness may terminate ONLY processes it launched itself. Ownership
is proven, never assumed:

* A launched process is recorded in a *lease file* together with its
  exact PID, image name and OS-reported creation time.
* Before any signal is sent, the entry is re-verified against the live
  OS (name AND creation time must both match). A stale or recycled PID,
  or any name disagreement, is skipped and reported — never killed.
* Webview2 children are owned via the run's WEBVIEW2_USER_DATA_FOLDER:
  only msedgewebview2 processes whose command line references that exact
  directory are touched.
* If the DevTools port is still occupied after cleanup, callers must
  FAIL with diagnostics. There is deliberately no fallback that could
  reach owner-controlled processes (Codex, OpenCode, Antigravity, IDEs,
  other WebView2 apps, ...).

Name-based killing (image-name flags, PowerShell name kills, psutil
name filters) is forbidden in this repository and enforced by the
static guard test `core/tests/qa_process_safety.rs`.
"""
from __future__ import annotations

import json
import socket
import subprocess
from pathlib import Path


class ProcessSafetyError(RuntimeError):
    """Raised when a port cannot be claimed without unsafe fallbacks."""


def _run_json(command: str) -> list[dict]:
    """Run a PowerShell snippet that emits JSON; tolerate no output."""
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command", command],
        capture_output=True,
        text=True,
        timeout=30,
    )
    text = (result.stdout or "").strip()
    if not text:
        return []
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        return []
    if isinstance(parsed, dict):
        return [parsed]
    return parsed if isinstance(parsed, list) else []


def _escape(value: str) -> str:
    return value.replace("'", "''")


def query_processes(name: str | None = None, cmdline_contains: str | None = None,
                    pids: list[int] | None = None) -> list[dict]:
    """Read-only process enumeration (never kills anything).

    Returns dicts with keys: pid (int), name (str), created_unix (int|None),
    cmdline (str). Filtering by exact image name, a command-line substring,
    or an explicit PID set — all of which are *identification* filters used
    to prove ownership before anything else happens.
    """
    clauses = []
    if name:
        clauses.append(f"Name = '{_escape(name)}'")
    if pids:
        # WQL IN(...) is unreliable across CIM versions; OR of equalities is
        # equivalent and universally supported.
        clauses.extend(f"ProcessId = {int(p)}" for p in pids)
    filter_arg = ""
    if clauses:
        filter_arg = '-Filter "' + " OR ".join(clauses).replace('"', '`"') + '" '
    select = (
        "@{n='pid';e={$_.ProcessId}},"
        "@{n='name';e={$_.Name}},"
        "@{n='created_unix';e={if ($_.CreationDate) { [DateTimeOffset]::new($_.CreationDate).ToUnixTimeSeconds() } else { $null }}},"
        "@{n='cmdline';e={$_.CommandLine}}"
    )
    where = ""
    if cmdline_contains:
        where = "| Where-Object { $_.cmdline -like '*" + _escape(cmdline_contains) + "*' }"
    out = _run_json(
        f"Get-CimInstance Win32_Process {filter_arg}| Select-Object {select}{where} "
        f"| ConvertTo-Json -Compress"
    )
    for row in out:
        row["pid"] = int(row.get("pid") or 0)
    return [row for row in out if row.get("pid")]


def _identity_of(pid: int) -> dict | None:
    rows = query_processes(pids=[int(pid)])
    return rows[0] if rows else None


def _kill_pid_tree(pid: int) -> None:
    """Kill one verified PID and its children. Callers MUST have verified
    ownership (name + creation time, or our own user-data-dir evidence)
    immediately before calling this. There is intentionally no name-based
    variant of this function anywhere in the repository."""
    subprocess.run(
        ["taskkill", "/F", "/T", "/PID", str(int(pid))],
        capture_output=True,
        timeout=15,
    )


class OwnershipLease:
    """Records the processes a harness launched so that a later run can
    clean up exactly those processes after re-verifying their identity.

    The lease file lives inside testing/native/ and is git-ignored; it is
    runtime state, never committed."""

    def __init__(self, lease_path: Path):
        self.lease_path = Path(lease_path)
        self.entries: list[dict] = []
        self.port: int | None = None
        self.user_data_dir: str | None = None
        self._load()

    def _load(self) -> None:
        self.entries = []
        self.port = None
        self.user_data_dir = None
        if not self.lease_path.exists():
            return
        try:
            data = json.loads(self.lease_path.read_text(encoding="utf-8"))
            self.entries = [e for e in data.get("entries", []) if e.get("pid")]
            self.port = data.get("port")
            self.user_data_dir = data.get("user_data_dir")
        except (json.JSONDecodeError, OSError):
            # A corrupt lease is treated as absent: cleanup then relies on
            # the port check, which fails loudly instead of guessing.
            self.entries = []

    def record(self, pid: int, name: str) -> None:
        """Register a process this harness launched. The creation time is
        captured from the OS immediately, so the identity can be re-proven
        later even after this run exits."""
        identity = _identity_of(pid) or {}
        if identity.get("name") and identity["name"].lower() != name.lower():
            raise ProcessSafetyError(
                f"refusing to record PID {pid}: OS reports '{identity.get('name')}' "
                f"but the harness launched '{name}'"
            )
        self.entries.append({
            "pid": int(pid),
            "name": name,
            "created_unix": identity.get("created_unix"),
        })
        self.save()

    def save(self) -> None:
        payload = {
            "entries": self.entries,
            "port": self.port,
            "user_data_dir": self.user_data_dir,
        }
        self.lease_path.parent.mkdir(parents=True, exist_ok=True)
        self.lease_path.write_text(json.dumps(payload, indent=1), encoding="utf-8")

    def clear(self) -> None:
        self.entries = []
        self.port = None
        self.user_data_dir = None
        try:
            if self.lease_path.exists():
                self.lease_path.unlink()
        except OSError:
            pass

    def release_previous(self) -> dict:
        """Clean up entries recorded by a *previous* run (a crashed run may
        have left its app + webview children alive).

        Every entry is re-verified against the live OS first:
          * process gone           -> nothing to do
          * name/creation mismatch -> SKIPPED (the PID belongs to someone
                                      else now — possibly the owner)
          * identity matches       -> killed by exact PID (with children)
        Webview2 leftovers are found via the recorded per-run
        WEBVIEW2_USER_DATA_FOLDER, never by image name alone.
        Returns a report; never raises for skipped entries."""
        report = {"killed": [], "skipped_stale": [], "already_gone": []}
        for entry in self.entries:
            pid = int(entry.get("pid") or 0)
            if not pid:
                continue
            live = _identity_of(pid)
            if live is None:
                report["already_gone"].append(entry)
                continue
            expected_name = str(entry.get("name") or "").lower()
            expected_created = entry.get("created_unix")
            name_ok = expected_name and str(live.get("name") or "").lower() == expected_name
            created_ok = (
                expected_created is None
                or live.get("created_unix") is None
                or abs(int(live["created_unix"]) - int(expected_created)) <= 2
            )
            if not (name_ok and created_ok):
                report["skipped_stale"].append({
                    "recorded": entry,
                    "live_now": {"name": live.get("name"), "created_unix": live.get("created_unix")},
                })
                continue
            _kill_pid_tree(pid)
            report["killed"].append(entry)
        # Webview2 children of a previous run: owned by the recorded
        # user-data-folder (unique per run), located by command line.
        if self.user_data_dir:
            marker = str(self.user_data_dir)
            for row in query_processes(name="msedgewebview2.exe",
                                       cmdline_contains=marker):
                _kill_pid_tree(row["pid"])
                report["killed"].append({"pid": row["pid"], "name": row["name"],
                                         "via": "user_data_dir"})
        self.clear()
        return report


def ensure_port_free(port: int) -> None:
    """FAIL with actionable diagnostics if the port is occupied. Deliberately
    offers no kill fallback: whatever holds the port may be owner-controlled,
    and requirement is that cleanup only ever touches verified own processes."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        if sock.connect_ex(("127.0.0.1", int(port))) != 0:
            return
    holders = query_processes()  # full table once, for the diagnostic only
    hint = ""
    for row in holders:
        cmdline = str(row.get("cmdline") or "")
        if f"--remote-debugging-port={int(port)}" in cmdline or f":{int(port)}" in cmdline:
            hint = (
                f" likely holder: PID {row['pid']} '{row.get('name')}' "
                f"(started {row.get('created_unix')}); command line: {cmdline[:160]}"
            )
            break
    raise ProcessSafetyError(
        f"CDP port {port} is still occupied after ownership-scoped cleanup. "
        "Refusing to launch: a stale or foreign instance holds the port. "
        "Resolve it manually (it may be owner-controlled — do NOT kill it by "
        f"image name).{hint}"
    )


def pre_launch_cleanup(lease: OwnershipLease, port: int) -> dict:
    """Standard pre-launch sequence for native harnesses: release whatever a
    previous run recorded, then insist on a free port."""
    report = lease.release_previous()
    ensure_port_free(port)
    return report

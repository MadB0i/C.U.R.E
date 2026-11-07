"""Non-destructive validation of the process-ownership checks (UI-02B).

Proves, without terminating anything:
  1. identity lookup matches the exact spawned PID/name/creation time;
  2. a lease entry whose recorded identity does NOT match the live process
     is classified stale and is NOT killed (the core safety property —
     the exact bug that killed the owner's Codex sessions);
  3. an already-exited lease entry is classified gone, not killed;
  4. webview2 scoping by user-data-folder matches nothing it shouldn't.

The only process spawned here is a scratch `cmd /c ping -n 3` that exits
by itself after ~2.5 s; the script never signals it. Run:

    python testing/native/qa_ownership_check.py
"""
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from process_ownership import (  # noqa: E402
    OwnershipLease,
    query_processes,
)

results = []


def check(name, ok, extra=""):
    results.append((name, ok))
    print(("PASS  " if ok else "FAIL  ") + name + (("  [" + extra + "]") if extra else ""))


def main():
    lease_path = Path(__file__).resolve().parent / ".process-lease-qa-check.json"
    # ~14 s lifetime: comfortably longer than the validation steps, and it
    # still exits BY ITSELF — nothing in this script ever signals it.
    scratch = subprocess.Popen(
        ["cmd", "/c", "ping", "-n", "15", "127.0.0.1", ">nul"],
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    try:
        # 1. Identity lookup matches exactly.
        rows = query_processes(pids=[scratch.pid])
        matched = [r for r in rows if r["pid"] == scratch.pid]
        check("identity lookup finds the scratch process by exact PID",
              len(matched) == 1 and matched[0]["name"].lower() == "cmd.exe",
              str([r["name"] for r in rows]))
        created = matched[0]["created_unix"] if matched else None
        check("creation time is reported by the OS",
              created is not None and abs(created - time.time()) < 120,
              str(created))

        # 2. Stale/mismatched lease entry: classification says skip, and the
        # process SURVIVES the release call (this is the Codex-kill bug,
        # proven fixed — without killing anything).
        stale_lease = OwnershipLease(lease_path)
        stale_lease.entries = [{
            "pid": scratch.pid,
            "name": "notepad.exe",       # wrong name
            "created_unix": 1000,        # wrong creation time
        }]
        report = stale_lease.release_previous()
        check("mismatched entry is classified stale, not killed",
              len(report["skipped_stale"]) == 1 and not report["killed"],
              str(report))
        still = query_processes(pids=[scratch.pid])
        check("the mismatched target was NOT terminated",
              len(still) == 1 and still[0]["pid"] == scratch.pid)
        check("stale classification carries actionable diagnostics",
              "recorded" in report["skipped_stale"][0]
              and "live_now" in report["skipped_stale"][0],
              str(report["skipped_stale"][0])[:90])

        # 3. Already-exited entry: classified gone, nothing killed.
        gone_lease = OwnershipLease(lease_path)
        gone_lease.entries = [{
            "pid": 4000000,              # implausible PID: nothing there
            "name": "cmd.exe",
            "created_unix": None,
        }]
        report = gone_lease.release_previous()
        check("already-gone entry is classified gone, not killed",
              len(report["already_gone"]) == 1 and not report["killed"],
              str(report))

        # 4. Webview2 scoping: a foreign/nonexistent user-data folder never
        # matches anyone's webview processes.
        foreign = query_processes(
            name="msedgewebview2.exe",
            cmdline_contains="D:\\definitely-not-a-real-cure-dir",
        )
        check("webview2 scoping matches nothing outside our own folder",
              foreign == [], str(foreign[:2]))

        # 5. The scratch process exits BY ITSELF (proof: no signal applied).
        deadline = time.time() + 15
        while time.time() < deadline and scratch.poll() is None:
            time.sleep(0.3)
        check("scratch process exited naturally (no termination applied)",
              scratch.poll() is not None and scratch.poll() == 0,
              "exit=" + str(scratch.poll()))
    finally:
        if scratch.poll() is None:
            print("FAIL  scratch process did not self-exit; it must be "
                  "reaped manually (PID %d) — this is a validation-script "
                  "bug, not a harness kill" % scratch.pid)
        lp = Path(__file__).resolve().parent / ".process-lease-qa-check.json"
        if lp.exists():
            lp.unlink()

    failed = [n for n, ok in results if not ok]
    print(f"\n{len(results) - len(failed)}/{len(results)} ownership checks passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

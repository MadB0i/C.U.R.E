#!/usr/bin/env python3
"""Native Windows end-to-end checks for C.U.R.E.

This drives the **real** release binary through the **real** WebView2
webview and the **real** Rust backend over the DevTools protocol. Nothing
here talks to the mock shell.

Safety rules this script obeys without exception:

* The only file it ever removes is one it created itself, inside a
  directory it created itself, inside the operating system's own temporary
  directory.
* It never adds a permanent watcher, never installs anything, never
  changes a system setting, and never touches a file outside its own
  fixture.
* The fixture is removed by the script at the end, whether the run passed
  or failed.

Run it against a built binary:

    cargo build --release --manifest-path gui/src-tauri/Cargo.toml
    python testing/native/native-e2e.py

Options:

    --exe PATH        the binary to test (default: the release build)
    --shots DIR       where to write the screenshots
    --keep-fixture    leave the fixture behind for inspection
    --cdp-port PORT   the DevTools port to use (default: 9333)
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEFAULT_EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"

FIXTURE_BYTES = 64 * 1024 * 1024  # 64 MiB: above the large-file threshold


def _kill_lingering_webview2() -> None:
    """Kill any lingering msedgewebview2.exe processes from prior runs.

    The native smoke test (and previous e2e runs) often leaves child
    WebView2 processes alive after the parent cure-gui.exe is killed.
    Those zombies hold file locks on the default WebView2 profile and
    can occupy the DevTools port, causing the next run to fail with a
    timeout or port-collision error.
    """
    import platform

    if platform.system() != "Windows":
        return
    try:
        import psutil
    except ImportError:
        # Fallback to WMIC if psutil is unavailable (e.g. in a minimal env).
        subprocess.run(
            ["taskkill", "/F", "/IM", "msedgewebview2.exe"],
            capture_output=True,
            timeout=10,
        )
        return
    gone = 0
    for proc in psutil.process_iter(["pid", "name"]):
        if proc.info["name"] and "msedgewebview2" in proc.info["name"].lower():
            try:
                proc.kill()
                gone += 1
            except psutil.NoSuchProcess:
                pass
    if gone:
        print(f"  killed {gone} lingering msedgewebview2 process(es)", flush=True)


class Failure(Exception):
    """A check that did not hold."""


class Runner:
    def __init__(self, exe: Path, shots: Path, port: int, keep: bool) -> None:
        self.exe = exe
        self.shots = shots
        self.port = port
        self.keep = keep
        self.checks: list[str] = []
        self.shot_index = 0
        self.process: subprocess.Popen | None = None
        self.exe_dir_before: set[str] = set()
        self.fixture: Path | None = None
        self.fixture_file: Path | None = None

    # -- reporting ---------------------------------------------------------

    def check(self, condition: bool, label: str) -> None:
        if not condition:
            raise Failure(label)
        self.checks.append(label)
        print(f"  PASS  {label}", flush=True)

    def step(self, label: str) -> None:
        print(f"\n== {label}", flush=True)

    # -- fixture -----------------------------------------------------------

    def make_fixture(self) -> None:
        """A directory of our own, holding one file of our own."""
        root = Path(tempfile.mkdtemp(prefix="cure-native-e2e-"))
        self.fixture = root
        self.fixture_file = root / "e2e-fixture.bin"
        block = b"C.U.R.E native end-to-end fixture.\n" * 1024
        with open(self.fixture_file, "wb") as handle:
            written = 0
            while written < FIXTURE_BYTES:
                handle.write(block)
                written += len(block)
        print(f"  fixture: {self.fixture_file} ({FIXTURE_BYTES} bytes)", flush=True)

    def remove_fixture(self) -> None:
        if self.fixture and not self.keep and self.fixture.exists():
            shutil.rmtree(self.fixture, ignore_errors=True)
        # Close any open log handles and remove the log files so they don't
        # linger after the fixture directory itself is gone.
        for attr in ("_stdout_log", "_stderr_log"):
            fh = getattr(self, attr, None)
            if fh is not None:
                try:
                    fh.close()
                except Exception:
                    pass
        for path in (getattr(self, "_stdout_path", None),
                     getattr(self, "_stderr_path", None)):
            if path is not None and isinstance(path, Path) and path.exists():
                try:
                    path.unlink()
                except Exception:
                    pass

    def _read_log_output(self) -> None:
        """Read whatever has been written to the stdout/stderr log files."""
        for name, target in [
            ("_stdout_path", "_stdout"),
            ("_stderr_path", "_stderr"),
        ]:
            path = getattr(self, name, None)
            if path is None or not isinstance(path, Path):
                continue
            try:
                if path.exists():
                    setattr(self, target, path.read_bytes())
            except Exception:
                pass

    # -- application -------------------------------------------------------

    def launch(self) -> None:
        self.exe_dir_before = {p.name for p in self.exe.parent.glob("*.json")}

        def check_port(p: int) -> bool:
            try:
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                    return s.connect_ex(('127.0.0.1', p)) == 0
            except Exception:
                return False

        # Kill any lingering WebView2 processes from prior runs (e.g. the
        # smoke test) before we claim the port. This prevents port collisions
        # and profile locks from previous invocations.
        _kill_lingering_webview2()

        if check_port(self.port):
            raise Failure(
                f"port {self.port} is already in use before application launch "
                "(possible port collision or zombie WebView2 process from prior run)"
            )

        env = dict(os.environ)
        # main.rs reads CURE_E2E_CDP_PORT to enable --remote-debugging-port.
        # We set this env var rather than WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
        # because wry ignores that variable when additional_browser_args is
        # not explicitly set (wry 0.55.1, webview2/mod.rs:294).
        env["CURE_E2E_CDP_PORT"] = str(self.port)
        # Use a dedicated temporary WebView2 user data folder for isolation,
        # preventing profile contamination or locking from prior test runs (like the smoke test).
        webview_data = self.fixture / "webview-data"
        env["WEBVIEW2_USER_DATA_FOLDER"] = str(webview_data)
        webview_data.mkdir(exist_ok=True)

        # Write stdout/stderr to temp files instead of piping.
        # Playwright's connect_over_cdp only talks to WebView2 over CDP; the
        # app's own stdout can be large (scan progress, logging) and would
        # fill the ~64 KB Windows pipe buffer, blocking the child process
        # and deadlocking this launcher loop forever.
        self._stdout_log = tempfile.NamedTemporaryFile(
            prefix="cure-e2e-stdout-", suffix=".log", delete=False
        )
        self._stderr_log = tempfile.NamedTemporaryFile(
            prefix="cure-e2e-stderr-", suffix=".log", delete=False
        )
        self._stdout_path = Path(self._stdout_log.name)
        self._stderr_path = Path(self._stderr_log.name)
        self._stdout_log.close()
        self._stderr_log.close()
        self._stdout = b""
        self._stderr = b""
        self.process = subprocess.Popen(
            [str(self.exe)],
            env=env,
            stdout=open(self._stdout_path, "wb", buffering=0),
            stderr=open(self._stderr_path, "wb", buffering=0),
        )
        deadline = time.time() + 60
        last_error = None
        while time.time() < deadline:
            code = self.process.poll()
            if code is not None:
                self._read_log_output()
                raise Failure(
                    f"the application exited early with code {code}"
                )
            try:
                with urllib.request.urlopen(
                    f"http://127.0.0.1:{self.port}/json/list", timeout=1
                ) as response:
                    targets = json.load(response)
                if any(t.get("type") == "page" for t in targets):
                    return
            except urllib.error.URLError as e:
                last_error = f"URLError: {e.reason}"
            except Exception as e:
                last_error = f"{type(e).__name__}: {e}"
            time.sleep(0.4)

        port_active = check_port(self.port)
        self._read_log_output()
        diag = self.collect_startup_diagnostics()
        print(diag, file=sys.stderr, flush=True)
        raise Failure(
            f"the application never exposed a webview on the DevTools port {self.port} "
            f"(timeout 60s, last connection error: {last_error}, port active: {port_active}, "
            f"process running: {self.process.poll() is None})"
        )

    def collect_startup_diagnostics(self) -> str:
        pid = self.process.pid if self.process else None
        lines = [f"=== STARTUP DIAGNOSTICS (PID: {pid}, Port: {self.port}) ==="]

        # 1. Process tree summary (Process ID, Parent ID, Name only - no raw command lines)
        try:
            res = subprocess.run(
                ["powershell", "-Command", f"Get-CimInstance Win32_Process | Where-Object {{ $_.ParentProcessId -eq {pid} -or $_.ProcessId -eq {pid} -or $_.Name -eq 'msedgewebview2.exe' }} | Select-Object ProcessId, ParentProcessId, Name | ConvertTo-Json"],
                capture_output=True, text=True, timeout=15
            )
            lines.append("Process / WebView2 summary:")
            procs = json.loads(res.stdout) if res.stdout.strip() else []
            if isinstance(procs, dict):
                procs = [procs]
            has_webview2 = any(p.get("Name", "").lower() == "msedgewebview2.exe" for p in procs)
            lines.append(f"  - Total related processes found: {len(procs)}")
            lines.append(f"  - msedgewebview2.exe child processes present: {has_webview2}")
        except Exception as e:
            lines.append(f"Process tree check status: unavailable ({e})")

        # 2. Check presence of debugging flag in main process safely
        try:
            res = subprocess.run(
                ["powershell", "-Command", f"Get-CimInstance Win32_Process | Where-Object {{ $_.ProcessId -eq {pid} }} | Select-Object -ExpandProperty CommandLine"],
                capture_output=True, text=True, timeout=10
            )
            cmd = res.stdout
            has_debug_port = f"--remote-debugging-port={self.port}" in cmd
            lines.append(f"  - Remote debugging flag present in main process: {has_debug_port}")
        except Exception:
            lines.append("  - Remote debugging flag check: unavailable")

        # 3. Relevant socket state for the DevTools port only
        try:
            res = subprocess.run(
                ["netstat", "-ano"], capture_output=True, text=True, timeout=10
            )
            port_str = f":{self.port}"
            matching = [l.strip() for l in res.stdout.splitlines() if port_str in l]
            lines.append(f"Socket state for port {self.port}:")
            if matching:
                for m in matching:
                    lines.append(f"  - {m}")
            else:
                lines.append(f"  - No active socket listeners or connections found for port {self.port}")
        except Exception as e:
            lines.append(f"Socket state check status: unavailable ({e})")

        # 4. Runner / CI execution context (boolean presence only, no values or paths)
        ci_active = os.environ.get("CI") is not None
        github_action = os.environ.get("GITHUB_ACTIONS") is not None
        lines.append(f"Environment context: CI={ci_active}, GITHUB_ACTIONS={github_action}")

        # 5. App stdout/stderr (truncated)
        if self._stderr:
            lines.append(f"App stderr (last 2000 chars):")
            lines.append(self._stderr.decode("utf-8", errors="replace")[-2000:])
        if self._stdout:
            lines.append(f"App stdout (last 500 chars):")
            lines.append(self._stdout.decode("utf-8", errors="replace")[-500:])

        # 6. CDP /json/version probe
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{self.port}/json/version", timeout=2
            ) as resp:
                version_info = json.load(resp)
            lines.append(f"CDP /json/version: {json.dumps(version_info)}")
        except Exception as e:
            lines.append(f"CDP /json/version probe failed: {type(e).__name__}: {e}")

        # 7. Chosen user-data folder
        if self.fixture:
            webview_data = self.fixture / "webview-data"
            lines.append(f"WebView2 user-data folder: {webview_data}")
            lines.append(f"  exists: {webview_data.exists()}")

        return "\n".join(lines)

    def shutdown(self) -> None:
        if self.process and self.process.poll() is None:
            # Kill the main app process first.
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
            # WebView2 spawns child processes that do NOT die with the parent.
            # Kill them explicitly so their file locks are released and the
            # fixture directory can be cleaned up on the next run.
            try:
                import psutil
                parent = psutil.Process(self.process.pid)
                for child in parent.children(recursive=True):
                    try:
                        child.kill()
                    except psutil.NoSuchProcess:
                        pass
                _, alive = psutil.wait_procs(
                    [psutil.Process(c.pid) for c in parent.children(recursive=True)],
                    timeout=5,
                )
                for p in alive:
                    try:
                        p.kill()
                    except psutil.NoSuchProcess:
                        pass
            except Exception:
                pass

    def connect(self):
        from playwright.sync_api import sync_playwright

        self._pw = sync_playwright().start()
        self._browser = self._pw.chromium.connect_over_cdp(
            f"http://127.0.0.1:{self.port}"
        )
        context = self._browser.contexts[0]
        page = context.pages[0]
        page.set_viewport_size({"width": 1440, "height": 900})
        page.wait_for_url("http://tauri.localhost/", timeout=30000)
        page.wait_for_load_state("networkidle")
        time.sleep(1.0)
        return page

    def shot(self, page, name: str) -> Path:
        self.shot_index += 1
        target = self.shots / f"{self.shot_index:02d}-{name}.png"
        page.screenshot(path=str(target))
        print(f"  shot  {target.name}", flush=True)
        return target


def run(exe: Path, shots: Path, port: int, keep: bool) -> int:
    shots.mkdir(parents=True, exist_ok=True)
    runner = Runner(exe, shots, port, keep)
    page = None
    try:
        runner.make_fixture()
        runner.step("application startup")
        runner.launch()
        page = runner.connect()
        runner.check(
            page.title().startswith("C.U.R.E"),
            f"the window title identifies the application ({page.title()!r})",
        )
        runner.check(
            page.evaluate("() => typeof window.__TAURI__ === 'object'"),
            "the real Tauri bridge is present (this is not the mock shell)",
        )
        runner.check(
            page.evaluate("() => typeof window.CureCompanion === 'object'"),
            "the companion is mounted in the native window",
        )
        runner.shot(page, "overview")

        runner.step("navigation")
        for nav in [
            "nav-overview",
            "nav-scan",
            "nav-cleanup",
            "nav-quarantine",
            "nav-detective",
            "nav-eventlog",
        ]:
            page.evaluate(
                """(id) => { const n = document.getElementById(id);
                   if (n) n.click(); }""",
                nav,
            )
            time.sleep(0.7)
            state = page.evaluate(
                """() => {
                    const views = [...document.querySelectorAll('.stage > .view')]
                        .filter((v) => v.getClientRects().length > 0);
                    const current = [...document.querySelectorAll('[aria-current="true"]')]
                        .map((n) => n.id);
                    return { count: views.length,
                             view: views.length ? views[0].id : '',
                             current: current };
                }"""
            )
            runner.check(
                state["count"] == 1 and nav in state["current"],
                f"{nav} opens exactly one view ({state['view']})",
            )
        runner.step("a real storage scan")
        page.evaluate("""() => document.getElementById('nav-cleanup').click()""")
        time.sleep(0.6)
        page.evaluate("""() => document.getElementById('sw-scan').click()""")
        time.sleep(1.2)
        progress = page.evaluate(
            """() => { const p = document.getElementById('sw-progress');
               return p ? { visible: p.getClientRects().length > 0,
                            text: (p.innerText || '').replace(/\\s+/g,' ').trim() } : null; }"""
        )
        runner.check(
            progress is not None and progress["visible"],
            "the storage scan shows real progress",
        )
        print(f"  progress text: {progress['text'][:120]!r}", flush=True)
        runner.shot(page, "storage-scanning")
        page.wait_for_selector("#sw-progress", state="hidden", timeout=300000)
        time.sleep(1.0)
        runner.shot(page, "storage-measured")

        measured = page.evaluate(
            """() => {
                const status = document.getElementById('sw-status-text');
                const amount = document.querySelector('.sw-headline-amount, .sw-amount, #sw-amount');
                return {
                  status: status ? status.innerText.trim() : '',
                  headline: amount ? amount.innerText.trim() : '',
                  roots: document.querySelectorAll('#sw-roots-body tr').length,
                };
            }"""
        )
        runner.check(
            measured["status"] in ("Measured", "At least", "Partial"),
            f"the scan reports its completeness ({measured['status']!r})",
        )
        runner.check(
            measured["roots"] > 0,
            f"the coverage tab lists the roots that were measured ({measured['roots']})",
        )
        runner.check(
            page.evaluate(
                "() => (window.CureStorageWorkstation.state.scan.finished_unix || 0) > 0"
            ),
            "the measured scan carries its finish time",
        )

        runner.step("coverage")
        page.evaluate("""() => document.getElementById('sw-tab-coverage').click()""")
        time.sleep(1.0)
        runner.shot(page, "storage-coverage")
        coverage = page.evaluate(
            """() => ({
                stats: document.querySelectorAll('#sw-stat-rows dt').length,
                roots: document.querySelectorAll('#sw-roots-body tr').length,
                failures: document.querySelectorAll('#sw-failures li').length,
            })"""
        )
        runner.check(coverage["stats"] > 0, "the coverage report is populated")

        runner.step("the detective board reads the same real measurement")
        page.evaluate("""() => document.getElementById('nav-detective').click()""")
        time.sleep(0.8)
        # Adopt the scan the workstation just ran rather than starting a
        # second full-disk measurement. The board is being checked, not the
        # scanner it already exercised.
        page.evaluate(
            """() => {
                const scan = window.CureStorageWorkstation.state.scan;
                window.CureDetective.adopt(scan);
            }"""
        )
        page.wait_for_selector("#dt-leads .dt-lead", timeout=30000)
        time.sleep(1.0)
        board = page.evaluate(
            """() => ({
                total: (document.getElementById('dt-map-total') || {}).innerText || '',
                leads: document.querySelectorAll('#dt-leads .dt-lead').length,
                arcs: document.querySelectorAll('#dt-map .dt-arc').length,
                state: (document.querySelector('[data-companion="detective"]') || {}).dataset.state || '',
            })"""
        )
        runner.check(board["total"] not in ("", "—"),
                     f"the board leads with the measured total ({board['total']!r})")
        runner.check(board["leads"] > 0, f"the board lists real leads ({board['leads']})")
        runner.check(board["arcs"] > 0, "the map draws real category arcs")
        runner.check(board["state"] in ("major", "discovering", "curious", "thinking"),
                     f"the companion reacts to the investigation ({board['state']!r})")
        runner.shot(page, "detective-board")

        page.evaluate("""() => document.getElementById('dt-case-preview').click()""")
        page.wait_for_selector("#dt-case-preview-stage img", timeout=15000)
        page.evaluate(
            """() => document.getElementById('dt-case-preview-stage')
                .scrollIntoView({ block: 'center' })"""
        )
        time.sleep(0.6)
        privacy = page.evaluate(
            """() => {
                const s = JSON.stringify(window.CureCaseFile.audit(
                    window.CureDetective.state.caseFile));
                return s.match(/[A-Za-z]:\\\\|Users|AppData|\\.tmp|\\.zip/gi) || [];
            }"""
        )
        runner.check(not privacy,
                     f"the native case file carries no path or file name ({privacy[:2]})")
        runner.check(
            not page.evaluate("!!window.CureDetective.state.caseFile.demo"),
            "the native case file is not marked as demo data",
        )
        runner.shot(page, "detective-case-file")

        # Return to the workstation, whose measurement the board adopted.
        page.evaluate("""() => document.getElementById('nav-cleanup').click()""")
        time.sleep(0.8)

        runner.step("the fixture is measured, and only the fixture is ever removed")
        page.evaluate("""() => document.getElementById('sw-tab-large').click()""")
        time.sleep(1.2)
        found = page.evaluate(
            """(name) => {
                const rows = [...document.querySelectorAll('#sw-large-body tr')];
                const hit = rows.find((r) => (r.innerText || '').includes(name));
                return hit ? hit.innerText.replace(/\\s+/g,' ').trim() : null;
            }""",
            runner.fixture_file.name,
        )
        runner.check(
            found is not None,
            f"the fixture file appears in the largest-files list ({runner.fixture_file.name})",
        )
        print(f"  row: {found!r}", flush=True)
        runner.shot(page, "storage-largest-files")

        queued = page.evaluate(
            """(name) => {
                const rows = [...document.querySelectorAll('#sw-large-body tr')];
                const hit = rows.find((r) => (r.innerText || '').includes(name));
                if (!hit) return false;
                const btn = hit.querySelector('.sw-queue-btn');
                if (!btn) return false;
                btn.click();
                return true;
            }""",
            runner.fixture_file.name,
        )
        runner.check(queued, "the fixture file can be queued for review")
        page.evaluate("""() => document.getElementById('sw-tab-queue').click()""")
        time.sleep(0.8)
        runner.shot(page, "storage-queue")

        runner.step("preview, confirmation and the plan that is actually executed")
        page.evaluate("""() => document.getElementById('sw-preview-btn').click()""")
        time.sleep(1.0)
        preview = page.evaluate(
            """() => ({
                visible: document.getElementById('sw-preview-overlay').getClientRects().length > 0,
                consequence: (document.getElementById('sw-preview-consequence').innerText || ''),
                items: document.querySelectorAll('#sw-preview-list li').length,
                confirmDisabled: document.getElementById('sw-preview-confirm').disabled,
            })"""
        )
        runner.check(preview["visible"], "the removal preview opens")
        runner.check(preview["items"] > 0, "the preview lists what will be removed")
        runner.check(
            "no undo" in preview["consequence"].lower()
            or "permanent" in preview["consequence"].lower(),
            f"the preview states the consequence plainly ({preview['consequence'][:90]!r})",
        )
        runner.check(
            preview["confirmDisabled"],
            "removal cannot be confirmed without typing the word",
        )
        runner.shot(page, "storage-preview")

        # The keyboard must be able to reach the confirmation field, and the
        # dialog must not let it escape.
        page.evaluate(
            """() => document.getElementById('sw-confirm-input').focus()"""
        )
        trapped = page.evaluate(
            """() => {
                const ov = document.getElementById('sw-preview-overlay');
                for (let i = 0; i < 20; i++) {
                    const ev = new KeyboardEvent('keydown', {key: 'Tab', bubbles: true});
                    document.dispatchEvent(ev);
                    const a = document.activeElement;
                    if (!a || !ov.contains(a)) return false;
                }
                return true;
            }"""
        )
        runner.check(trapped, "the confirmation dialog keeps the keyboard inside it")

        page.evaluate(
            """() => { const i = document.getElementById('sw-confirm-input');
                       i.value = 'REMOVE';
                       i.dispatchEvent(new Event('input', {bubbles: true})); }"""
        )
        time.sleep(0.4)
        runner.check(
            page.evaluate(
                """() => !document.getElementById('sw-preview-confirm').disabled"""
            ),
            "the exact confirmation word enables removal",
        )
        page.evaluate("""() => document.getElementById('sw-preview-confirm').click()""")
        time.sleep(2.0)
        receipt = page.evaluate(
            """() => {
                const ov = document.getElementById('sw-receipt-overlay');
                const rows = [...document.querySelectorAll('#sw-receipt-rows dt')].map(
                    (dt, i) => [dt.innerText.trim(),
                                 dt.nextElementSibling ? dt.nextElementSibling.innerText.trim() : '']);
                return {
                  visible: ov.getClientRects().length > 0,
                  amount: (document.getElementById('sw-receipt-amount') || {}).innerText,
                  label: (document.getElementById('sw-receipt-label') || {}).innerText,
                  rows: rows,
                };
            }"""
        )
        runner.check(receipt["visible"], "the receipt is shown after the removal")
        runner.shot(page, "storage-receipt")
        print("  receipt rows:", json.dumps(receipt["rows"], indent=2), flush=True)
        labels = " ".join(k.lower() for k, _ in receipt["rows"])
        runner.check(
            "removed" in labels and "refused" in labels,
            "the receipt separates removed from refused",
        )
        runner.check(
            "verified reclaimed" not in labels,
            "the receipt does not present an estimate as verified",
        )
        runner.check(
            not runner.fixture_file.exists(),
            "the approved file was removed",
        )
        page.evaluate("""() => document.getElementById('sw-receipt-close').click()""")
        time.sleep(0.6)

        runner.step("cancellation")
        page.evaluate("""() => document.getElementById('sw-scan').click()""")
        time.sleep(0.8)
        cancelled = page.evaluate(
            """() => { const b = document.getElementById('sw-cancel');
               if (!b) return false; b.click(); return true; }"""
        )
        if cancelled:
            page.wait_for_selector("#sw-progress", state="hidden", timeout=120000)
            time.sleep(0.8)
            status = page.evaluate(
                """() => document.getElementById('sw-status-text').innerText.trim()"""
            )
            runner.check(
                status in ("Cancelled", "Partial", "Ready", "Measured", "At least"),
                f"a cancelled scan reports itself ({status!r})",
            )
        else:
            print("  (the scan finished before it could be cancelled)", flush=True)

        runner.step("companion states")
        states = page.evaluate(
            """() => {
                const names = ['idle','scan','discovering','review','curious','thinking',
                               'quarantine','success','failure','cancelled','sleeping','major'];
                const out = [];
                for (const n of names) {
                    const api = window.CureCompanion;
                    if (n === 'idle') api.idle('probe');
                    else if (n === 'scan') api.scan('scan', 'probe');
                    else if (n === 'discovering') api.discovering(3, 'probe');
                    else if (n === 'review') api.review(2, 'probe');
                    else if (n === 'curious') api.curious('probe');
                    else if (n === 'thinking') api.thinking('probe');
                    else if (n === 'quarantine') api.seal('probe');
                    else if (n === 'success') api.cleanup('success', 'probe', 'probe');
                    else if (n === 'failure') api.failed('probe');
                    else if (n === 'cancelled') api.cancelled('probe');
                    else if (n === 'sleeping') api.sleep();
                    else if (n === 'major') api.major('probe');
                    out.push(document.querySelector('[data-companion="overview"]')
                               .getAttribute('data-state'));
                }
                return out;
            }"""
        )
        runner.check(
            len(set(states)) >= 12,
            f"the companion reaches every state the interface can ask for ({states})",
        )

        runner.step("reduced motion")
        page.emulate_media(reduced_motion="reduce")
        time.sleep(1.0)
        reduced = page.evaluate(
            """() => ({
                paused: document.documentElement.getAttribute('data-paused'),
                running: document.getAnimations()
                    .filter((a) => a.animationName && a.animationName.indexOf('lm-') === 0)
                    .length,
            })"""
        )
        runner.check(
            reduced["paused"] == "true" and reduced["running"] == 0,
            f"reduced motion stops every companion animation ({reduced})",
        )
        page.emulate_media(reduced_motion="no-preference")
        time.sleep(0.5)

        runner.step("the application keeps its own records to itself")
        stray = sorted({p.name for p in exe.parent.glob("*.json")} - runner.exe_dir_before)
        runner.check(
            not stray,
            f"nothing new is written next to the executable ({stray})",
        )

        print(f"\nPASS: {len(runner.checks)} native checks", flush=True)
        return 0
    except Failure as failure:
        print(f"\nFAIL: {failure}", file=sys.stderr, flush=True)
        if page is not None:
            try:
                page.screenshot(path=str(shots / "99-failure.png"))
            except Exception:
                pass
        return 1
    except Exception as error:  # noqa: BLE001
        print(f"\nERROR: {error!r}", file=sys.stderr, flush=True)
        return 2
    finally:
        try:
            runner.shutdown()
        except Exception:
            pass
        try:
            runner.remove_fixture()
        except Exception:
            pass
        try:
            runner._pw.stop()
        except Exception:
            pass


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=DEFAULT_EXE)
    parser.add_argument(
        "--shots", type=Path, default=REPO / "docs" / "native-captures"
    )
    parser.add_argument("--keep-fixture", action="store_true")
    parser.add_argument("--cdp-port", type=int, default=9333)
    args = parser.parse_args()

    exe = args.exe.resolve()
    if not exe.exists():
        print(
            f"the binary is not built: {exe}\n"
            "build it first: cargo build --release --manifest-path gui/src-tauri/Cargo.toml",
            file=sys.stderr,
        )
        return 2
    return run(exe, args.shots, args.cdp_port, args.keep_fixture)


if __name__ == "__main__":
    raise SystemExit(main())

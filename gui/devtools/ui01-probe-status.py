import os, subprocess, tempfile, time, urllib.request, json
from pathlib import Path
from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
PORT = 9403

def wait_cdp(timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=1)
            return
        except Exception:
            time.sleep(0.5)
    raise SystemExit("no cdp")

env = dict(os.environ)
env["CURE_E2E_CDP_PORT"] = str(PORT)
tmp = tempfile.TemporaryDirectory(prefix="cure-probe-", ignore_cleanup_errors=True)
env["WEBVIEW2_USER_DATA_FOLDER"] = str(Path(tmp.name) / "wv")
proc = subprocess.Popen([str(EXE)], env=env)
try:
    wait_cdp()
    with sync_playwright() as p:
        b = p.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
        page = b.contexts[0].pages[0]
        page.wait_for_load_state("networkidle")
        out = page.evaluate("window.__TAURI__.core.invoke('operator_status')")
        print(json.dumps(out, indent=1)[:1200])
        b.close()
finally:
    proc.terminate()
    try: proc.wait(timeout=10)
    except Exception: proc.kill()
    tmp.cleanup()

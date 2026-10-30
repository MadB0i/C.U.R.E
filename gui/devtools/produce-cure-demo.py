import os, sys, time, subprocess, tempfile, json, ctypes, urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui/src-tauri/target/release/cure-gui.exe"
MEDIA = REPO / "docs/media"
MEDIA.mkdir(parents=True, exist_ok=True)
RAW_VIDEO = MEDIA / "raw-cure-demo.mp4"
FINAL_VIDEO = MEDIA / "cure-demo.mp4"
PREVIEW_GIF = MEDIA / "cure-preview.gif"
POSTER_PNG = MEDIA / "cure-poster.png"

PORT = 9485

user32 = ctypes.windll.user32
user32.SetProcessDPIAware()

def find_hwnd(title_part):
    found = []
    def enum_cb(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            length = user32.GetWindowTextLengthW(hwnd)
            buff = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, buff, length + 1)
            if title_part in buff.value:
                found.append(hwnd)
        return True
    WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_int, ctypes.c_int)
    user32.EnumWindows(WNDENUMPROC(enum_cb), 0)
    return found[0] if found else None

def record_journey():
    temp_dir = tempfile.TemporaryDirectory(prefix="cure-rec-")
    fixture = Path(temp_dir.name)
    webview_data = fixture / "webview-data"
    webview_data.mkdir()

    # Create a safe test workspace folder for the Flight Recorder scene
    rec_ws = fixture / "workspace"
    rec_ws.mkdir()
    (rec_ws / "agent-config.json").write_text('{"name": "test-agent"}', encoding="utf-8")

    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(PORT)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(webview_data)

    print("Step 1: Launching cure-gui.exe with CDP enabled...")
    proc = subprocess.Popen([str(EXE)], env=env)

    try:
        deadline = time.time() + 30
        hwnd = None
        while time.time() < deadline:
            hwnd = find_hwnd("C.U.R.E")
            if hwnd:
                break
            time.sleep(0.4)

        if not hwnd:
            raise RuntimeError("Could not find C.U.R.E window")
        print(f"Window found (HWND: {hwnd}). Positioning window to 1600x900...")
        user32.SetWindowPos(hwnd, 0, 40, 40, 1600, 900, 0x0040)
        user32.SetForegroundWindow(hwnd)
        time.sleep(2.0)

        # Wait for CDP endpoint
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/list", timeout=1) as resp:
                    targets = json.load(resp)
                if any(t.get("type") == "page" for t in targets):
                    break
            except Exception:
                time.sleep(0.4)

        print("Step 2: Starting FFmpeg gdigrab on C.U.R.E window...")
        if RAW_VIDEO.exists():
            RAW_VIDEO.unlink()

        ffmpeg_cmd = [
            "ffmpeg", "-y", "-hide_banner", "-loglevel", "warning",
            "-f", "gdigrab", "-framerate", "30",
            "-draw_mouse", "0",
            "-i", "title=C.U.R.E — Evidence Console",
            "-vf", "crop=trunc(iw/2)*2:trunc(ih/2)*2,scale=1920:1080:flags=lanczos,format=yuv420p",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18",
            "-pix_fmt", "yuv420p",
            str(RAW_VIDEO)
        ]
        rec = subprocess.Popen(ffmpeg_cmd, stdin=subprocess.PIPE)
        time.sleep(1.2)

        print("Step 3: Driving the native application through the storyboard...")
        from playwright.sync_api import sync_playwright
        with sync_playwright() as pw:
            b = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
            page = b.contexts[0].pages[0]

            # Scene 1 (0–5s): Overview & LUMA
            print("  [0-5s] Scene 1: Overview & LUMA animation")
            page.locator("#nav-overview").click()
            page.wait_for_selector("#view-overview:not(.hidden)", timeout=5000)
            time.sleep(4.5)

            # Scene 2 (5–14s): Holographic Rescue Scanner
            print("  [5-14s] Scene 2: Holographic Rescue Scanner")
            page.locator("#nav-scan").click()
            page.wait_for_selector("#scan-view:not(.hidden)", timeout=5000)
            time.sleep(1.8)
            # Start scan
            page.locator("#xray-start-btn").click()
            time.sleep(8.0)

            # Scene 3 (14–23s): AI Agent Radar V3
            print("  [14-23s] Scene 3: AI Agent Radar V3 & Evidence Inspector")
            page.locator("#nav-agents").click()
            page.wait_for_selector("#view-agents:not(.hidden)", timeout=5000)
            time.sleep(2.0)
            # Inspect first agent card if present
            cards = page.locator("#ar-agent-list .ar-card")
            if cards.count() > 0:
                cards.first.click()
                time.sleep(3.5)
                close_btn = page.locator("#ar-insp-close")
                if close_btn.is_visible():
                    close_btn.click()
                    time.sleep(1.0)
            else:
                time.sleep(4.5)

            # Scene 4 (23–30s): Flight Recorder
            print("  [23-30s] Scene 4: Agent Flight Recorder")
            page.locator("#ar-jump-monitor").click()
            time.sleep(1.0)
            ws_input = page.locator("#ar-workspace")
            ws_input.fill(str(rec_ws))
            time.sleep(1.0)
            page.locator("#ar-start").click()
            time.sleep(1.5)
            # Create a file event in monitored workspace
            (rec_ws / "session_event.py").write_text("# automated agent task\n", encoding="utf-8")
            time.sleep(2.5)
            # Stop recording
            page.locator("#ar-stop").click()
            # Wait for case preview button to become enabled
            try:
                page.wait_for_function("() => { const b = document.getElementById('ar-case-preview'); return b && !b.disabled; }", timeout=5000)
            except Exception:
                pass
            time.sleep(1.5)

            # Scene 5 (30–37s): Windows Change Radar & Case File
            print("  [30-37s] Scene 5: Windows Change Radar & Case File Generator")
            page.locator("#ar-win-compare").click()
            time.sleep(2.5)
            case_input = page.locator("#ar-case-summary")
            if case_input.is_visible():
                case_input.fill("Verified clean session with active developer agent processes")
                page.locator("#ar-case-include").check()
                time.sleep(1.0)
                try:
                    prev_btn = page.locator("#ar-case-preview")
                    if prev_btn.is_enabled():
                        prev_btn.click()
                        time.sleep(3.0)
                except Exception as e:
                    print("  Note on case preview:", e)
                    time.sleep(3.0)

            # Scene 6 (37–43s): Overview Finale
            print("  [37-43s] Scene 6: Overview Finale & LUMA Closing")
            page.locator("#nav-overview").click()
            page.wait_for_selector("#view-overview:not(.hidden)", timeout=5000)
            time.sleep(5.0)

            b.close()

        print("Step 4: Stopping FFmpeg recording...")
        try:
            rec.stdin.write(b"q\n")
            rec.stdin.flush()
            rec.wait(timeout=5)
        except Exception:
            rec.terminate()
            rec.wait(timeout=5)

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()
        subprocess.run(["taskkill", "/F", "/IM", "cure-gui.exe", "/T"], capture_output=True)
        try:
            temp_dir.cleanup()
        except Exception:
            pass

    print("Raw capture finished! Output size:", RAW_VIDEO.stat().st_size if RAW_VIDEO.exists() else 0)

    # Step 5: Post-process Final Demo MP4 (H.264, web-optimized faststart, crisp 1080p, < 12 MB)
    print("Step 5: Post-processing Final Demo MP4...")
    if FINAL_VIDEO.exists():
        FINAL_VIDEO.unlink()
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-i", str(RAW_VIDEO),
        "-c:v", "libx264", "-preset", "slow", "-crf", "22",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        str(FINAL_VIDEO)
    ], check=True)
    print(f"Final MP4 created: {FINAL_VIDEO} ({FINAL_VIDEO.stat().st_size / (1024*1024):.2f} MB)")

    # Step 6: Create Animated GIF (7.5 seconds from the Holographic Rescue Scanner, 860px wide, < 3 MB)
    print("Step 6: Creating Premium Animated GIF from Holographic Rescue Scanner...")
    if PREVIEW_GIF.exists():
        PREVIEW_GIF.unlink()
    # High-quality 2-pass palette generation for smooth GIF colors
    palette = MEDIA / "palette.png"
    # Scanner scene is around 6s - 14s in the video
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-ss", "00:00:06.5", "-t", "7.5",
        "-i", str(FINAL_VIDEO),
        "-vf", "fps=15,scale=860:-1:flags=lanczos,palettegen=stats_mode=diff",
        str(palette)
    ], check=True)
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-ss", "00:00:06.5", "-t", "7.5",
        "-i", str(FINAL_VIDEO),
        "-i", str(palette),
        "-lavfi", "fps=15,scale=860:-1:flags=lanczos [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=3",
        str(PREVIEW_GIF)
    ], check=True)
    if palette.exists():
        palette.unlink()
    print(f"Preview GIF created: {PREVIEW_GIF} ({PREVIEW_GIF.stat().st_size / (1024*1024):.2f} MB)")

    # Step 7: Create Poster PNG
    print("Step 7: Creating Video Poster Frame...")
    if POSTER_PNG.exists():
        POSTER_PNG.unlink()
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-ss", "00:00:09.0",
        "-i", str(FINAL_VIDEO),
        "-vframes", "1",
        str(POSTER_PNG)
    ], check=True)
    print(f"Poster created: {POSTER_PNG} ({POSTER_PNG.stat().st_size / 1024:.1f} KB)")

if __name__ == "__main__":
    record_journey()

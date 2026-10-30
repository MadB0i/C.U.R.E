import os, sys, time, threading, subprocess, tempfile, json, base64, shutil
from pathlib import Path
from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui/src-tauri/target/release/cure-gui.exe"
MEDIA = REPO / "docs/media"
MEDIA.mkdir(parents=True, exist_ok=True)
RAW_VIDEO = MEDIA / "raw-cure-demo.mp4"
FINAL_VIDEO = MEDIA / "cure-demo.mp4"
PREVIEW_GIF = MEDIA / "cure-preview.gif"
POSTER_PNG = MEDIA / "cure-poster.png"

PORT = 9488

def record_journey():
    temp_dir = tempfile.TemporaryDirectory(prefix="cure-rec-")
    fixture = Path(temp_dir.name)
    webview_data = fixture / "webview-data"
    webview_data.mkdir()

    # Create safe privacy-neutral test workspace for Flight Recorder
    rec_ws = Path("D:/CURE_DEMO_WORKSPACE")
    if rec_ws.exists():
        shutil.rmtree(rec_ws, ignore_errors=True)
    rec_ws.mkdir(parents=True, exist_ok=True)
    (rec_ws / "agent-config.json").write_text('{"name": "test-agent"}', encoding="utf-8")

    env = dict(os.environ)
    env["CURE_E2E_CDP_PORT"] = str(PORT)
    env["WEBVIEW2_USER_DATA_FOLDER"] = str(webview_data)

    print("Step 1: Launching cure-gui.exe with CDP enabled...")
    proc = subprocess.Popen([str(EXE)], env=env)

    if RAW_VIDEO.exists():
        RAW_VIDEO.unlink()

    ffmpeg_cmd = [
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "warning",
        "-f", "image2pipe", "-vcodec", "mjpeg", "-framerate", "30",
        "-i", "-",
        "-c:v", "libx264", "-preset", "medium", "-crf", "19",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        str(RAW_VIDEO)
    ]
    ffmpeg_proc = subprocess.Popen(ffmpeg_cmd, stdin=subprocess.PIPE)

    current_frame = [None]
    recording = [True]

    def writer_worker():
        interval = 1.0 / 30.0
        next_time = time.time()
        while recording[0]:
            frame = current_frame[0]
            if frame:
                try:
                    ffmpeg_proc.stdin.write(frame)
                except Exception:
                    break
            next_time += interval
            sleep_dur = next_time - time.time()
            if sleep_dur > 0:
                time.sleep(sleep_dur)
            else:
                next_time = time.time()

    writer_thread = threading.Thread(target=writer_worker, daemon=True)

    try:
        time.sleep(3.0)
        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(f"http://127.0.0.1:{PORT}")
            page = browser.contexts[0].pages[0]
            page.set_viewport_size({"width": 1920, "height": 1080})
            page.wait_for_load_state("networkidle")

            cdp = browser.contexts[0].new_cdp_session(page)

            def handle_frame(event):
                current_frame[0] = base64.b64decode(event["data"])
                try:
                    cdp.send("Page.screencastFrameAck", {"sessionId": event["sessionId"]})
                except Exception:
                    pass

            cdp.on("Page.screencastFrame", handle_frame)
            cdp.send("Page.startScreencast", {
                "format": "jpeg",
                "quality": 95,
                "maxWidth": 1920,
                "maxHeight": 1080,
                "everyNthFrame": 1
            })

            # Wait for first frame to arrive
            for _ in range(60):
                if current_frame[0] is not None:
                    break
                page.wait_for_timeout(50)

            writer_thread.start()
            print("Step 2: Recording 6-scene application journey...")

            # Helper for smooth event pumping
            def wait_seconds(secs):
                end = time.time() + secs
                while time.time() < end:
                    page.wait_for_timeout(50)

            # Scene 1 (0–6s): Brand Introduction & Overview with animated LUMA
            print("  [0-6s] Scene 1: Overview & LUMA Companion")
            page.click("#nav-overview")
            wait_seconds(5.0)

            # Scene 2 (6–16s): Holographic Rescue Scanner
            print("  [6-16s] Scene 2: Holographic Rescue Scanner (live sweep & rings)")
            page.click("#nav-scan")
            wait_seconds(2.0)
            page.click("#xray-start-btn")
            wait_seconds(3.5)
            page.screenshot(path=str(MEDIA / "cure-rescue-scanner.png"))
            wait_seconds(4.5)

            # Scene 3 (16–25s): AI Agent Radar V3
            print("  [16-25s] Scene 3: AI Agent Radar V3 & Process Intelligence")
            page.click("#nav-agents")
            wait_seconds(2.5)
            # Click an agent row/card to open Process Inspector if present
            try:
                card = page.locator(".ar-agent-card, .ar-agent-row, [data-agent-pid]").first
                if card.is_visible():
                    card.click()
                    wait_seconds(3.0)
                    close_btn = page.locator("#ar-insp-close")
                    if close_btn.is_visible():
                        close_btn.click()
                        wait_seconds(1.0)
                else:
                    wait_seconds(3.0)
            except Exception:
                wait_seconds(3.0)

            # Scene 4 (25–33s): Agent Flight Recorder
            print("  [25-33s] Scene 4: Agent Flight Recorder (workspace timeline)")
            try:
                ws_input = page.locator("#ar-workspace")
                if ws_input.is_visible():
                    ws_input.fill(str(rec_ws))
                    wait_seconds(0.5)
                    start_rec = page.locator("#ar-start")
                    if start_rec.is_enabled():
                        start_rec.click()
                        wait_seconds(2.0)
                        # Simulate a safe file event in the test workspace
                        (rec_ws / "build.log").write_text("Compiling sub-module\nDone", encoding="utf-8")
                        wait_seconds(2.0)
                        stop_rec = page.locator("#ar-stop")
                        if stop_rec.is_enabled():
                            stop_rec.click()
                            wait_seconds(2.0)
            except Exception as e:
                print("  Note on Flight Recorder:", e)
                wait_seconds(4.0)

            # Scene 5 (33–40s): Windows Change Radar & Case File
            print("  [33-40s] Scene 5: Windows Change Radar & Case File Generator")
            try:
                page.click("#ar-win-compare")
                wait_seconds(2.5)
                case_input = page.locator("#ar-case-summary")
                if case_input.is_visible():
                    case_input.fill("Verified clean session with active developer agent processes")
                    page.locator("#ar-case-include").check()
                    wait_seconds(1.0)
                    prev_btn = page.locator("#ar-case-preview")
                    if prev_btn.is_enabled():
                        prev_btn.click()
                        wait_seconds(3.5)
            except Exception as e:
                print("  Note on Windows Changes:", e)
                wait_seconds(4.0)

            # Scene 6 (40–46s): Overview Finale & LUMA Closing
            print("  [40-46s] Scene 6: Overview Finale & LUMA Closing")
            page.click("#nav-overview")
            wait_seconds(5.0)

            cdp.send("Page.stopScreencast")
            recording[0] = False
            writer_thread.join(timeout=2)
            ffmpeg_proc.stdin.close()
            ffmpeg_proc.wait(timeout=5)
            print("Capture finished! Raw video size:", RAW_VIDEO.stat().st_size)

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
        if rec_ws.exists():
            shutil.rmtree(rec_ws, ignore_errors=True)

    # Step 3: Optimize Final Demo MP4 (H.264, 1080p, CRF 20, < 12 MB)
    print("Step 3: Optimizing Final Demo MP4...")
    if FINAL_VIDEO.exists():
        FINAL_VIDEO.unlink()
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-i", str(RAW_VIDEO),
        "-c:v", "libx264", "-preset", "medium", "-crf", "20",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        str(FINAL_VIDEO)
    ], check=True)
    if RAW_VIDEO.exists():
        RAW_VIDEO.unlink()
    print(f"Final MP4 ready: {FINAL_VIDEO} ({FINAL_VIDEO.stat().st_size / (1024*1024):.2f} MB)")

    # Step 4: Create Animated GIF from the live Holographic Rescue Scanner (around 7s - 15s)
    print("Step 4: Creating Premium Animated GIF (live scanner sweep)...")
    if PREVIEW_GIF.exists():
        PREVIEW_GIF.unlink()
    palette = MEDIA / "palette.png"
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-ss", "00:00:08.0", "-t", "7.5",
        "-i", str(FINAL_VIDEO),
        "-vf", "fps=15,scale=860:-1:flags=lanczos,palettegen=stats_mode=diff",
        str(palette)
    ], check=True)
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-ss", "00:00:08.0", "-t", "7.5",
        "-i", str(FINAL_VIDEO),
        "-i", str(palette),
        "-lavfi", "fps=15,scale=860:-1:flags=lanczos [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=3",
        str(PREVIEW_GIF)
    ], check=True)
    if palette.exists():
        palette.unlink()
    print(f"Preview GIF ready: {PREVIEW_GIF} ({PREVIEW_GIF.stat().st_size / (1024*1024):.2f} MB)")

    # Step 5: Create Poster PNG
    print("Step 5: Creating Poster Frame...")
    if POSTER_PNG.exists():
        POSTER_PNG.unlink()
    subprocess.run([
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-ss", "00:00:11.0",
        "-i", str(FINAL_VIDEO),
        "-vframes", "1",
        str(POSTER_PNG)
    ], check=True)
    print(f"Poster ready: {POSTER_PNG} ({POSTER_PNG.stat().st_size / 1024:.1f} KB)")

if __name__ == "__main__":
    record_journey()

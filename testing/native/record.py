#!/usr/bin/env python3
"""Record the native C.U.R.E window with ffmpeg, while driving it.

This is a genuine desktop capture: ffmpeg's `gdigrab` grabs the real
application window from the screen, at the screen's own resolution, with
the real application running. Nothing is re-rendered, nothing is
composited, and no stills are stitched together.

The window is found through the *process*, not through its title: a
browser tab can carry the same title, and capturing the wrong window is
worse than capturing nothing. Every other visible window is minimised for
the duration of the recording and restored afterwards, so the frame can
only ever contain the application.

The script drives the window over the DevTools protocol while the capture
runs, so the recording shows the real application reacting in real time.

    python testing/native/record.py luma         # the companion showcase
    python testing/native/record.py walkthrough  # the storage journey
    python testing/native/record.py shots        # the acceptance captures
    python testing/native/record.py luma --exe path/to/other.exe
"""

from __future__ import annotations

import argparse
import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "gui" / "src-tauri" / "target" / "release" / "cure-gui.exe"
OUT = REPO / "docs" / "native-captures"
PORT = 9355
FFMPEG = os.environ.get("FFMPEG", "ffmpeg")

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

HWND_TOPMOST = -1
SWP_NOMOVE = 0x0002
SWP_NOSIZE = 0x0001
SWP_SHOWWINDOW = 0x0040
SWP_NOTOPMOST = 0x0010
SW_MAXIMIZE = 3
SW_MINIMIZE = 6
SW_RESTORE = 9


def become_dpi_aware() -> None:
    """Make this process per-monitor DPI aware before it asks the screen how
    big it is.

    A DPI-unaware process is handed *virtualised* coordinates: on a 125 %
    display it believes the screen is 1536x864 when it is really
    1920x1080. Capturing that rectangle would silently crop the
    right-hand quarter of the application out of the recording.
    """
    try:
        user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
    except Exception:
        try:
            ctypes.windll.shcore.SetProcessDpiAwareness(2)
        except Exception:
            pass


def app_hwnd(pid: int) -> int:
    """The application's own window handle, found through its process id."""
    found: list[int] = []

    def callback(hwnd, _lparam):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
            return False
        return True

    user32.EnumWindows(
        ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)(callback), 0
    )
    if not found:
        raise RuntimeError("the C.U.R.E window was not found")
    return found[0]


def window_rect(hwnd: int) -> tuple[int, int, int, int]:
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    return rect.left, rect.top, rect.right, rect.bottom


def client_rect(hwnd: int) -> tuple[int, int, int, int]:
    """The client area of the window, in screen coordinates."""
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = wintypes.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    return (origin.x, origin.y, rect.right - rect.left, rect.bottom - rect.top)


def maximize(hwnd: int) -> None:
    """Fill the screen, so the whole layout is in frame."""
    user32.ShowWindow(hwnd, SW_MAXIMIZE)
    user32.SetForegroundWindow(hwnd)


def other_windows(app_pid: int) -> list[int]:
    """Every other visible top-level window on this desktop."""
    found: list[int] = []

    def callback(other, _lparam):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(other, ctypes.byref(owner))
        if owner.value == app_pid or not user32.IsWindowVisible(other):
            return True
        if not user32.GetWindowTextLengthW(other):
            return True
        found.append(other)
        return True

    user32.EnumWindows(
        ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)(callback), 0
    )
    return found


def set_minimized(hwnd: int, minimized: bool) -> None:
    user32.ShowWindow(hwnd, SW_MINIMIZE if minimized else SW_RESTORE)


def isolate(hwnd: int, app_pid: int) -> list[int]:
    """Minimise everything else, so the recording contains only the app.

    The capture is of the *screen*, so anything on top of the application
    would be recorded too — including whatever the operator happens to have
    open. The windows are restored as soon as the recording is finished.
    """
    hidden: list[int] = []
    for other in other_windows(app_pid):
        if other == hwnd:
            continue
        try:
            if user32.IsIconic(other):
                continue
            set_minimized(other, True)
            hidden.append(other)
        except Exception:
            continue
    return hidden


def restore(hidden: list[int]) -> None:
    for other in hidden:
        try:
            set_minimized(other, False)
        except Exception:
            continue


def ensure_foreground(hwnd: int, attempts: int = 12) -> bool:
    """Put the application genuinely in front, and prove it.

    `SetForegroundWindow` is refused for a process that does not own the
    foreground, so the window is raised with `AttachThreadInput`, which
    lets this thread borrow the foreground thread's input queue.
    """
    for _ in range(attempts):
        user32.SetWindowPos(
            hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW
        )
        user32.SetForegroundWindow(hwnd)
        if user32.GetForegroundWindow() == hwnd:
            return True
        current = kernel32.GetCurrentThreadId()
        target = user32.GetWindowThreadProcessId(user32.GetForegroundWindow(), None)
        if target and target != current:
            user32.AttachThreadInput(current, target, True)
            user32.BringWindowToTop(hwnd)
            user32.SetForegroundWindow(hwnd)
            user32.AttachThreadInput(current, target, False)
        if user32.GetForegroundWindow() == hwnd:
            return True
        time.sleep(0.25)
    return False


def wait_for_window(pid: int, timeout: float = 60.0) -> int:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            return app_hwnd(pid)
        except RuntimeError:
            time.sleep(0.4)
    raise RuntimeError("the C.U.R.E window never appeared")


def ffmpeg_available() -> bool:
    try:
        subprocess.run(
            [FFMPEG, "-version"], capture_output=True, check=True, timeout=30
        )
        return True
    except Exception:
        return False


# ---------------------------------------------------------------------------
# Timelines
# ---------------------------------------------------------------------------

# The companion showcase: every authored state, in the order an operator
# would see them, with a real pet in the middle.
LUMA_SCRIPT = [
    # The character is shown large enough to judge, in a fixed place, so
    # the capture can be cut to it exactly. The application is still the
    # real one; only the companion is moved and enlarged.
    (
        "js",
        """() => {
        const h = document.querySelector('[data-companion="overview"]');
        if (!h) return;
        h.style.setProperty('--luma-size', '600px');
        h.style.position = 'fixed';
        h.style.left = '700px';
        h.style.top = '110px';
        h.style.width = '600px';
        h.style.zIndex = '40';
        const fig = h.querySelector('.luma-figure');
        if (fig) { fig.style.maxWidth = 'none'; }
      }""",
    ),
    ("js", "() => document.getElementById('nav-overview').click()"),
    ("wait", 2.0),
    ("js", "() => window.CureCompanion.idle('Idle. Breathing, blinking.')"),
    ("wait", 4.0),
    ("js", "() => window.CureCompanion.curious('Curious. What is that?')"),
    ("wait", 3.0),
    ("js", "() => window.CureCompanion.scan('scan', 'Scanning the machine.')"),
    ("wait", 4.0),
    ("js", "() => window.CureCompanion.discovering(3, 'Three findings.')"),
    ("wait", 2.5),
    ("js", "() => window.CureCompanion.thinking('Thinking it over.')"),
    ("wait", 3.0),
    (
        "js",
        """() => {
        const svg = document.querySelector('[data-companion="overview"] .luma-svg');
        const shape = svg.querySelector('.lm-body-shape');
        const r = shape.getBoundingClientRect();
        shape.dispatchEvent(new MouseEvent('click', {
          bubbles: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2}));
      }""",
    ),
    ("wait", 2.5),
    ("js", "() => window.CureCompanion.review(2, 'Two to review.')"),
    ("wait", 2.5),
    ("js", "() => window.CureCompanion.cleanup('cleanup', 'Sorting', 'Two files sorted.')"),
    ("wait", 2.5),
    ("js", "() => window.CureCompanion.seal('Sealing the file.')"),
    ("wait", 2.5),
    (
        "js",
        "() => window.CureCompanion.cleanup('success', 'All clear!', 'Everything checked out.')",
    ),
    ("wait", 3.5),
    ("js", "() => window.CureCompanion.cancelled('Stopped before the end.')"),
    ("wait", 2.5),
    ("js", "() => window.CureCompanion.failed('Needs a look.')"),
    ("wait", 2.5),
    # The fold: the standing cat goes down, and the curled one takes its
    # place. The two drawings are never on screen together.
    ("js", "() => window.CureCompanion.sleep()"),
    ("wait", 7.0),
    ("js", "() => window.CureCompanion.wake()"),
    ("wait", 2.5),
]

# The operator journey, against the real backend.
WALKTHROUGH_SCRIPT = [
    ("js", "() => document.getElementById('nav-overview').click()"),
    ("wait", 1.5),
    ("shot", "01-overview"),
    ("js", "() => document.getElementById('nav-scan').click()"),
    ("wait", 1.5),
    ("shot", "02-rescue"),
    ("js", "() => document.getElementById('nav-cleanup').click()"),
    ("wait", 1.5),
    ("shot", "03-storage-idle"),
    ("js", "() => document.getElementById('sw-scan').click()"),
    ("wait", 2.0),
    ("shot", "04-storage-scanning"),
    (
        "waitfor",
        "() => (document.getElementById('sw-status-text')||{}).textContent === 'Measured'",
        300,
    ),
    ("wait", 1.4),
    ("shot", "05-storage-measured"),
    ("js", "() => document.getElementById('sw-tab-map').click()"),
    ("wait", 1.8),
    ("shot", "06-storage-map"),
    ("js", "() => document.getElementById('sw-tab-large').click()"),
    ("wait", 1.4),
    ("shot", "07-storage-largest"),
    ("js", "() => document.getElementById('sw-tab-dupes').click()"),
    ("wait", 1.4),
    ("shot", "08-storage-duplicates"),
    ("js", "() => document.getElementById('sw-tab-coverage').click()"),
    ("wait", 1.4),
    ("shot", "09-storage-coverage"),
]

# The acceptance captures: each step leaves the application in a state
# worth looking at.
SHOTS_SCRIPT = [
    ("js", "() => document.getElementById('nav-overview').click()"),
    ("wait", 1.6),
    ("shot", "01-overview-with-luma"),
    (
        "js",
        """() => {
        const s = document.querySelector('[data-companion="overview"] .luma-hide');
        if (s) s.click();
      }""",
    ),
    ("wait", 1.2),
    ("shot", "02-overview-luma-hidden"),
    ("js", "() => document.getElementById('nav-cleanup').click()"),
    ("wait", 1.4),
    ("shot", "03-storage-idle"),
    ("js", "() => document.getElementById('sw-scan').click()"),
    ("wait", 1.6),
    ("shot", "04-storage-scanning"),
    (
        "waitfor",
        "() => (document.getElementById('sw-status-text')||{}).textContent === 'Measured'",
        300,
    ),
    ("wait", 1.4),
    ("shot", "05-storage-completed"),
    ("js", "() => document.getElementById('sw-tab-map').click()"),
    ("wait", 1.8),
    ("shot", "06-storage-treemap"),
    ("js", "() => document.getElementById('sw-tab-large').click()"),
    ("wait", 1.4),
    ("shot", "07-storage-largest-files"),
    ("js", "() => document.getElementById('sw-tab-dupes').click()"),
    ("wait", 1.4),
    ("shot", "08-storage-duplicates"),
    ("js", "() => document.getElementById('sw-tab-coverage').click()"),
    ("wait", 1.4),
    ("shot", "09-storage-coverage"),
    ("js", "() => document.getElementById('nav-eventlog').click()"),
    ("wait", 1.4),
    ("shot", "10-display-preferences"),
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["luma", "walkthrough", "shots"])
    parser.add_argument("--fps", type=int, default=30)
    parser.add_argument(
        "--exe",
        default=str(EXE),
        help="the binary to record (the public demo uses its own build)",
    )
    parser.add_argument("--port", type=int, default=PORT)
    parser.add_argument(
        "--crop",
        default=None,
        help="a selector for the region to report, so the capture can be cut down to it",
    )
    args = parser.parse_args()

    become_dpi_aware()
    if not ffmpeg_available():
        print("ffmpeg is not on PATH; set FFMPEG to its path", file=sys.stderr)
        return 2
    OUT.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (
        f"--remote-debugging-port={args.port}"
    )
    app = subprocess.Popen([args.exe], env=env)
    capture = None
    hidden: list[int] = []
    try:
        hwnd = wait_for_window(app.pid)
        time.sleep(1.5)
        maximize(hwnd)
        # Give the webview time to follow the window resize and re-lay out.
        time.sleep(4.0)
        hidden = isolate(hwnd, app.pid)
        time.sleep(1.0)
        if not ensure_foreground(hwnd):
            print(
                "warning: the application could not be brought to the front; "
                "the recording may show another window",
                file=sys.stderr,
            )
        left, top, right, bottom = window_rect(hwnd)
        cx, cy, cw, ch = client_rect(hwnd)
        print(
            f"window hwnd={hwnd}: frame {right-left}x{bottom-top} at ({left},{top}); "
            f"client {cw}x{ch} at ({cx},{cy})",
            flush=True,
        )

        screen_w = user32.GetSystemMetrics(0)
        screen_h = user32.GetSystemMetrics(1)
        capture_left = max(0, min(left, screen_w - 16))
        capture_top = max(0, min(top, screen_h - 16))
        capture_width = min(right - left, screen_w - capture_left)
        capture_height = min(bottom - top, screen_h - capture_top)
        print(
            f"capture: {capture_width}x{capture_height} at "
            f"({capture_left},{capture_top}) of a {screen_w}x{screen_h} screen",
            flush=True,
        )

        raw = OUT / f"raw-{args.mode}.mp4"
        if raw.exists():
            raw.unlink()
        capture = subprocess.Popen(
            [
                FFMPEG,
                "-hide_banner",
                "-loglevel", "error",
                "-f", "gdigrab",
                "-framerate", str(args.fps),
                "-offset_x", str(capture_left),
                "-offset_y", str(capture_top),
                "-video_size", f"{capture_width}x{capture_height}",
                "-i", "desktop",
                "-c:v", "libx264",
                "-preset", "veryfast",
                "-crf", "20",
                "-pix_fmt", "yuv420p",
                "-movflags", "+faststart",
                str(raw),
            ],
            stdin=subprocess.PIPE,
        )
        time.sleep(2.0)

        from playwright.sync_api import sync_playwright

        with sync_playwright() as p:
            browser = p.chromium.connect_over_cdp(
                f"http://127.0.0.1:{args.port}"
            )
            page = browser.contexts[0].pages[0]
            # The page is NOT resized from here: a CDP viewport override
            # changes the emulated size but not the real WebView2 surface.
            time.sleep(1.5)

            script = {
                "luma": LUMA_SCRIPT,
                "walkthrough": WALKTHROUGH_SCRIPT,
                "shots": SHOTS_SCRIPT,
            }[args.mode]
            for step in script:
                kind = step[0]
                if kind == "wait":
                    time.sleep(step[1])
                elif kind == "js":
                    page.evaluate(step[1])
                elif kind == "click":
                    try:
                        page.click(step[1], timeout=4000)
                    except Exception:
                        pass
                elif kind == "waitfor":
                    deadline = time.time() + step[2]
                    while time.time() < deadline:
                        if page.evaluate(step[1]):
                            break
                        time.sleep(0.25)
                elif kind == "shot":
                    page.screenshot(
                        path=str(OUT / f"video-{args.mode}-{step[1]}.png")
                    )
            # The region to cut the capture down to, if one was asked for.
            # The capture starts at the window's client area, so the offset
            # of the element inside the capture is the element's position
            # inside the client area.
            if args.crop:
                box = page.evaluate(
                    """(sel) => {
                        const n = document.querySelector(sel);
                        const r = n ? n.getBoundingClientRect() : null;
                        return {
                          x: r ? r.left : 0, y: r ? r.top : 0,
                          w: r ? r.width : 0, h: r ? r.height : 0,
                          vw: window.innerWidth, vh: window.innerHeight,
                          dpr: window.devicePixelRatio,
                        };
                    }""",
                    args.crop,
                )
                if box:
                    # The webview's CSS pixels are not the screen's pixels:
                    # a 125% display scales the whole page. The scale is the
                    # client area's width divided by the viewport's width.
                    scale = 1.0
                    if box["vw"]:
                        scale = cw / box["vw"]
                    print(
                        "CROP {0} {1} {2} {3} scale={4:.4f} client={5}x{6} at ({7},{8})".format(
                            int(box["x"]),
                            int(box["y"]),
                            int(box["w"]),
                            int(box["h"]),
                            scale,
                            cw,
                            ch,
                            cx,
                            cy,
                        ),
                        flush=True,
                    )
                else:
                    print(f"CROP not found: {args.crop}", flush=True)
            browser.close()
        time.sleep(1.0)
    finally:
        try:
            restore(hidden)
        except Exception:
            pass
        if capture is not None:
            try:
                capture.stdin.write(b"q")
                capture.stdin.flush()
            except Exception:
                pass
            try:
                capture.wait(timeout=20)
            except Exception:
                capture.kill()
        try:
            app.terminate()
            app.wait(timeout=10)
        except Exception:
            app.kill()

    print(f"raw capture: {OUT / f'raw-{args.mode}.mp4'}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

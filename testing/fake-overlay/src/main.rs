// ============================================================================
//  CURE TEST FIXTURE — NOT REAL MALWARE
//
//  Purpose: a plain, inert, always-on-top fullscreen window that visually
//  simulates a "ransom-style" screen lock so the CURE live-fire test can
//  verify that cure-watch still detects the trigger USB and that cure-gui
//  surfaces itself ABOVE this overlay.
//
//  What it does:   shows one borderless topmost window with static text.
//  What it is NOT: no file access, no encryption, no network, no registry,
//                  no persistence, nothing but a visible window.
//
//  Close it with Alt+F4 (or `taskkill /IM fake-overlay.exe /F`).
//  Source of truth lives next to this file; review before running anything.
// ============================================================================

#[cfg(not(target_os = "windows"))]
fn main() {
    println!(
        "fake-overlay is a Windows-only C.U.R.E test fixture; nothing to do on this platform."
    );
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(err) = run() {
        eprintln!("fake-overlay failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
const HEADLINE: &str = "SIMULATED RANSOM SCREEN — TEST FIXTURE ONLY";

#[cfg(target_os = "windows")]
const BODY_TEXT: &str = "This window exists purely to test cure-gui surfacing.\r\n\
It is NOT malware: no file access, no encryption, no network.\r\n\
Insert a C.U.R.E rescue USB — cure-gui must appear above this window.";

#[cfg(target_os = "windows")]
fn run() -> windows::core::Result<()> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateSolidBrush, DrawTextW, EndPaint, FillRect, SelectObject,
        SetBkMode, SetTextColor, HBRUSH, HFONT, PAINTSTRUCT,
    };
    use windows::Win32::Graphics::Gdi::{
        CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
        DT_NOPREFIX, DT_VCENTER, FW_BOLD, OUT_DEFAULT_PRECIS, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
        GetSystemMetrics, LoadCursorW, PostQuitMessage, RegisterClassW, SetWindowPos, ShowWindow,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, IDC_ARROW, MSG, SM_CXSCREEN,
        SM_CYSCREEN, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOW, WINDOW_STYLE, WM_DESTROY,
        WM_PAINT, WNDCLASSW, WS_EX_TOPMOST, WS_POPUP,
    };

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_PAINT => unsafe {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);

                let bg: HBRUSH = CreateSolidBrush(COLORREF(0x000026));
                let _ = FillRect(hdc, &rc, bg);
                let _ = SetBkMode(hdc, TRANSPARENT);
                let _ = SetTextColor(hdc, COLORREF(0x00FFFFFF));

                let height = rc.bottom - rc.top;
                let big = title_font(-height / 8);
                let small = title_font(-height / 28);
                let _ = SelectObject(hdc, big);

                let mut headline: Vec<u16> = HEADLINE.encode_utf16().collect();
                headline.push(0);
                let mut head_rc = rc;
                head_rc.bottom = rc.top + height * 2 / 5;
                DrawTextW(
                    hdc,
                    &mut headline,
                    &mut head_rc,
                    DT_CENTER | DT_VCENTER | DT_NOPREFIX,
                );

                let _ = SelectObject(hdc, small);
                let _ = SetTextColor(hdc, COLORREF(0x00B0FF));
                let mut body: Vec<u16> = BODY_TEXT.encode_utf16().collect();
                body.push(0);
                let mut body_rc = rc;
                body_rc.top = rc.top + height * 11 / 20;
                body_rc.bottom = rc.top + height * 4 / 5;
                DrawTextW(
                    hdc,
                    &mut body,
                    &mut body_rc,
                    DT_CENTER | DT_VCENTER | DT_NOPREFIX,
                );

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            },
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    #[allow(non_snake_case)]
    unsafe fn title_font(px: i32) -> HFONT {
        unsafe {
            CreateFontW(
                px,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            )
        }
    }

    unsafe {
        let hmodule = GetModuleHandleW(None)?;
        let hinstance = hmodule.into();
        let class_name: PCWSTR = w!("CureTestOverlayClass");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: CreateSolidBrush(COLORREF(0x000026)),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            class_name,
            w!("CURE TEST FIXTURE — NOT REAL MALWARE"),
            WINDOW_STYLE(WS_POPUP.0),
            0,
            0,
            width,
            height,
            None,
            None,
            hinstance,
            None,
        )?;

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

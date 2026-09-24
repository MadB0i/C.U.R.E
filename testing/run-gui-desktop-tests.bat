@echo off
REM Run the GUI desktop-dependent tests locally (interactive session only).
REM The overlay dismissal test spawns real windows (fake-overlay + notepad)
REM and closes one of them. NEVER run this on a machine you care about
REM without reading testing/fake-overlay/src/main.rs first; a clean VM with
REM a snapshot is the recommended host. CI runs only the headless-safe GUI
REM tests (plain `cargo test` skips #[ignore] tests by default).
setlocal
cd /d "%~dp0fake-overlay" || exit /b 1
cargo build --release || exit /b 1
cd /d "%~dp0..\gui\src-tauri" || exit /b 1
cargo test -- --ignored --nocapture

#Requires -Version 5.1
<#
.SYNOPSIS
  Builds (if needed) and launches CURE_TEST_overlay.exe: a tiny UNSIGNED,
  borderless, topmost test window that closes itself after N minutes.

.DESCRIPTION
  Why this shape: cure-gui's overlay review (Start Rescue ->
  `list_overlay_candidates`, per-window cards) lists a visible window only
  when ALL of these hold (`core/src/overlay.rs`): WS_EX_TOPMOST set,
  WS_CAPTION unset ("borderless"), owner binary resolvable and NOT
  ValidSigned, not cure-gui's own window, not under %WINDIR%, covering >=
  90% of its monitor, and not on the path+hash allowlist.
  The window below is WS_POPUP (no caption) + WS_EX_TOPMOST, lives in
  %TEMP%\CURE_TEST (not under %WINDIR%), and is compiled locally by csc.exe
  so it is UNSIGNED — but it is SMALL (~480x200, <25% of 1080p), so the
  coverage gate correctly EXCLUDES it. Use it as the negative control
  (must never be listed); use the repo's fullscreen testing/fake-overlay
  for the positive case. Closing is always per-window confirmed: Close =
  graceful WM_CLOSE (this window's DefWindowProc handles it), Force close =
  explicit terminate only.
  The window quits itself after -Minutes via a timer. No network, no files
  (besides its own exe), no registry, no persistence.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File CURE_TEST_overlay.ps1 -DryRun
  powershell -NoProfile -ExecutionPolicy Bypass -File CURE_TEST_overlay.ps1 -Minutes 5
#>
[CmdletBinding()]
param(
    [int]$Minutes = 5,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Prefix   = 'CURE_TEST_'
$TestDir  = Join-Path $env:TEMP 'CURE_TEST'
$ExeName  = 'CURE_TEST_overlay.exe'
$ExePath  = Join-Path $TestDir $ExeName
if (-not $ExeName.StartsWith($Prefix)) { throw "REFUSAL: bad exe name." }

$cscCandidates = @(
    "$env:SystemRoot\Microsoft.NET\Framework64\v4.0.30319\csc.exe",
    "$env:SystemRoot\Microsoft.NET\Framework\v4.0.30319\csc.exe"
)

$CsSource = @'
using System;
using System.Runtime.InteropServices;
static class CURE_TEST_Overlay {
    const int WS_POPUP = unchecked((int)0x80000000);
    const int WS_VISIBLE = 0x10000000;
    const int WS_EX_TOPMOST = 0x00000008;
    const int WM_DESTROY = 0x0002;
    const int WM_TIMER = 0x0113;
    const int SW_SHOW = 5;
    static readonly IntPtr HWND_TOPMOST = new IntPtr(-1);
    const uint SWP_NOMOVE = 0x0002, SWP_NOSIZE = 0x0001, SWP_SHOWWINDOW = 0x0040;
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASS { public uint style; public IntPtr lpfnWndProc; public int cbClsExtra, cbWndExtra;
        public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; }
    [StructLayout(LayoutKind.Sequential)]
    struct POINT { public int x, y; }
    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public POINT pt; }
    delegate IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern ushort RegisterClassW(ref WNDCLASS lpWndClass);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateWindowExW(
        int dwExStyle, string lpClassName, string lpWindowName, int dwStyle,
        int x, int y, int w, int h, IntPtr hParent, IntPtr hMenu, IntPtr hInst, IntPtr lpParam);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr hWnd, IntPtr hAfter,
        int x, int y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] static extern IntPtr SetTimer(IntPtr hWnd, IntPtr nIDEvent, uint uElapse, IntPtr lpTimerFunc);
    [DllImport("user32.dll")] static extern bool GetMessageW(out MSG lpMsg, IntPtr hWnd, uint wMin, uint wMax);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG lpMsg);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG lpMsg);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr hWnd);
    [DllImport("user32.dll")] static extern void PostQuitMessage(int nExitCode);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string lpModuleName);
    static int autoCloseMs;
    static IntPtr OnMsg(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam) {
        if (msg == WM_TIMER) { DestroyWindow(hWnd); return IntPtr.Zero; }
        if (msg == WM_DESTROY) { PostQuitMessage(0); return IntPtr.Zero; }
        return DefWindowProcW(hWnd, msg, wParam, lParam);
    }
    static int Main(string[] args) {
        autoCloseMs = args.Length > 0 ? int.Parse(args[0]) * 60 * 1000 : 5 * 60 * 1000;
        WndProc proc = OnMsg;
        GC.KeepAlive(proc);
        WNDCLASS wc = new WNDCLASS();
        wc.lpfnWndProc = Marshal.GetFunctionPointerForDelegate(proc);
        wc.hInstance = GetModuleHandleW(null);
        wc.lpszClassName = "CURE_TEST_OverlayClass";
        RegisterClassW(ref wc);
        IntPtr hwnd = CreateWindowExW(WS_EX_TOPMOST, wc.lpszClassName,
            "CURE_TEST overlay \u2014 harmless test window (auto-closes)",
            WS_POPUP | WS_VISIBLE, 200, 200, 480, 200,
            IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        if (hwnd == IntPtr.Zero) return 1;
        ShowWindow(hwnd, SW_SHOW);
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetTimer(hwnd, (IntPtr)1, (uint)autoCloseMs, IntPtr.Zero);
        MSG m;
        while (GetMessageW(out m, IntPtr.Zero, 0, 0)) { TranslateMessage(ref m); DispatchMessageW(ref m); }
        return 0;
    }
}
'@

Write-Host "CURE_TEST overlay window (unsigned, borderless, topmost, self-closing)"
Write-Host "Target exe: $ExePath"
Write-Host "Auto-close: $Minutes minute(s)"
Write-Host "DryRun    : $DryRun"

if ($DryRun) {
    Write-Host "[DRY-RUN] would: create $TestDir"
    Write-Host "[DRY-RUN] would: compile embedded C# with csc.exe -> $ExePath"
    Write-Host "[DRY-RUN] would: Start-Process $ExePath $Minutes"
    Write-Host "[DRY-RUN] done: nothing compiled, nothing launched."
    return
}

New-Item -ItemType Directory -Path $TestDir -Force | Out-Null
if (-not (Test-Path -LiteralPath $ExePath)) {
    $csc = $cscCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ($null -eq $csc) {
        throw "SKIP: no csc.exe found (tried Framework64/Framework v4.0.30319). Install .NET Framework or SDK to build the dummy."
    }
    Write-Host "compiling with: $csc"
    $src = Join-Path $TestDir 'CURE_TEST_overlay.cs'
    Set-Content -LiteralPath $src -Value $CsSource -Encoding UTF8
    & $csc /nologo /target:winexe /out:$ExePath $src
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $ExePath)) {
        throw "compilation failed (exit $LASTEXITCODE)"
    }
    Remove-Item -LiteralPath $src -Force
    Write-Host "built     : $ExePath (unsigned, verify: Get-AuthenticodeSignature shows NotSigned)"
} else {
    Write-Host "reusing   : $ExePath"
}

$sig = Get-AuthenticodeSignature -LiteralPath $ExePath
Write-Host "signature : $($sig.Status) (expected NotSigned — required for the overlay matcher)"
if ($sig.Status -ne 'NotSigned') {
    throw "REFUSAL: $ExePath is unexpectedly signed ($($sig.Status)); not launching."
}

$p = Start-Process -FilePath $ExePath -ArgumentList @("$Minutes") -PassThru
Write-Host "launched  : pid $($p.Id); window auto-closes after $Minutes minute(s)."
Write-Host "next      : in cure-gui click Start Rescue and watch for the graceful close."

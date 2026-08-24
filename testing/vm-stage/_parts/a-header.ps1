# =============================================================================
#  C.U.R.E real-VM validation - in-guest evidence collector (TEST-ONLY)
#
#  Implements steps 3-9 of TESTING.md inside the VirtualBox guest.
#  Run from PowerShell inside the GUEST VM:
#
#    powershell -NoProfile -ExecutionPolicy Bypass -File C:\cure-test\run-cure-validation.ps1 -Phase Setup
#      ... then REBOOT the VM (Start > Restart), log back in, then IMMEDIATELY:
#    powershell -NoProfile -ExecutionPolicy Bypass -File C:\cure-test\run-cure-validation.ps1 -Phase PostReboot
#      ... then:
#    powershell -NoProfile -ExecutionPolicy Bypass -File C:\cure-test\run-cure-validation.ps1 -Phase OverlayAndUSB
#
#  Do not start cure-watch.exe manually at any point. The whole point is that
#  after the reboot ONLY the Startup-folder copy may be running.
# =============================================================================

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Setup', 'PostReboot', 'OverlayAndUSB')]
    [string]$Phase,

    [string]$Stage = '\\VBoxSVR\cure-stage',
    [string]$GuestDir = 'C:\cure-test'
)

$ErrorActionPreference = 'Continue'
$WatchLog   = Join-Path $env:APPDATA 'cure-watch.log'
$StartupExe = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\cure-watch.exe'
$StateFile  = Join-Path $GuestDir 'state.json'
$Stamp      = Get-Date -Format 'yyyyMMdd-HHmmss'

if (-not (Test-Path "$Stage\cure-watch.exe")) {
    Write-Host "FATAL: stage share '$Stage' not reachable. Is the shared folder mounted?"
    exit 1
}

New-Item -ItemType Directory -Path $GuestDir -Force | Out-Null
$EvidenceLocal = Join-Path $GuestDir 'evidence'
New-Item -ItemType Directory -Path $EvidenceLocal -Force | Out-Null

function Save-Evidence([string]$Name, [string]$Text) {
    $local = Join-Path $EvidenceLocal "$Stamp-$Name.txt"
    Set-Content -Path $local -Value $Text -Encoding UTF8
    try {
        New-Item -ItemType Directory -Path "$Stage\cure-evidence" -Force | Out-Null
        Copy-Item $local (Join-Path "$Stage\cure-evidence" "$Stamp-$Name.txt") -Force
    } catch { Write-Host "note: could not mirror evidence to stage share ($($_.Exception.Message))" }
    Write-Host "evidence written: $local"
}

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WEnum {
  public struct Info { public IntPtr Hwnd; public string Title; public bool Visible; public bool Topmost; public int L, T, R, B; }
  static List<Info> _list = new List<Info>();
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern int GetWindowLongW(IntPtr h, int i);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static List<Info> Snap() {
    _list = new List<Info>();
    EnumWindows((h, l) => {
      var sb = new StringBuilder(512);
      GetWindowTextW(h, sb, 512);
      RECT r;
      GetWindowRect(h, out r);
      _list.Add(new Info { Hwnd = h, Title = sb.ToString(), Visible = IsWindowVisible(h),
        Topmost = (GetWindowLongW(h, -20) & 0x8) != 0,
        L = r.Left, T = r.Top, R = r.Right, B = r.Bottom });
      return true;
    }, IntPtr.Zero);
    return _list;
  }
}
'@

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class Pix {
  [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
}
'@

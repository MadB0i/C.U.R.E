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


function Test-OverlayColor([uint32]$c) {
    return (($c -band 0xFF) -eq 38 -and ((($c -shr 8) -band 0xFF) -eq 0) -and ((($c -shr 16) -band 0xFF) -eq 0))
}

function Get-ZRows([string]$Filter) {
    $rows = @()
    $i = 0
    foreach ($w in [WEnum]::Snap()) {
        if ($w.Title -match $Filter) {
            $rows += [pscustomobject]@{
                Z = $i; Title = $w.Title; Vis = $w.Visible; Topmost = $w.Topmost
                Rect = '{0},{1} {2}x{3}' -f $w.L, $w.T, ($w.R - $w.L), ($w.B - $w.T)
                L = $w.L; T = $w.T; W = ($w.R - $w.L); H = ($w.B - $w.T)
            }
        }
        $i++
    }
    return $rows
}

function Parse-StartupEntries {
    if (-not (Test-Path $WatchLog)) { return @() }
    $out = @()
    foreach ($line in Get-Content $WatchLog) {
        if ($line -match '^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})Z \[startup\] watcher started \(pid (\d+)') {
            $ts = [datetime]::ParseExact($Matches[1], 'yyyy-MM-ddTHH:mm:ss',
                    [System.Globalization.CultureInfo]::InvariantCulture,
                    [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal)
            $out += [pscustomobject]@{ Utc = $ts; Pid = [int]$Matches[2]; Raw = $line }
        }
    }
    return $out
}

function Save-Screenshot([string]$Name) {
    Add-Type -AssemblyName System.Drawing
    Add-Type -AssemblyName System.Windows.Forms
    $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap($vs.Width, $vs.Height)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($vs.Left, $vs.Top, 0, 0, $bmp.Size)
    $g.Dispose()
    $path = Join-Path $EvidenceLocal "$Stamp-$Name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    try {
        New-Item -ItemType Directory -Path "$Stage\cure-evidence" -Force | Out-Null
        Copy-Item $path (Join-Path "$Stage\cure-evidence" "$Stamp-$Name.png") -Force
    } catch {}
    Write-Host "screenshot: $path"
}

function Test-GuiAboveOverlay {
    $rows = Get-ZRows 'TEST FIXTURE|Clean USB Rescue'
    $gui = $rows | Where-Object { $_.Vis -and $_.Title -match 'Clean USB Rescue' } | Select-Object -First 1
    $ovl = $rows | Where-Object { $_.Vis -and $_.Title -match 'TEST FIXTURE' } | Select-Object -First 1
    $L = @()
    if (-not $ovl) { return @{ Ok = $false; Log = @('OVERLAY WINDOW NOT FOUND'); GuiRect = '' } }
    if (-not $gui)  { return @{ Ok = $false; Log = @('GUI WINDOW NOT FOUND');      GuiRect = '' } }
    $L += ('ZORDER gui Z={0} topmost={1} rect={2}' -f $gui.Z, $gui.Topmost, $gui.Rect)
    $L += ('ZORDER overlay Z={0} topmost={1} rect={2}' -f $ovl.Z, $ovl.Topmost, $ovl.Rect)
    $zOk = $gui.Z -lt $ovl.Z

    $dc = [Pix]::GetDC([IntPtr]::Zero)
    $hits = 0; $total = 0
    for ($iy = 0; $iy -lt 5; $iy++) {
        for ($ix = 0; $ix -lt 7; $ix++) {
            $x = [int]($gui.L + 40 + $ix * (($gui.W - 80) / 6.0))
            $y = [int]($gui.T + 30 + $iy * (($gui.H - 60) / 4.0))
            $total++
            if (Test-OverlayColor ([Pix]::GetPixel($dc, $x, $y))) { $hits++ }
        }
    }
    $vsW = [System.Windows.Forms.SystemInformation]::VirtualScreen.Width
    $vsH = [System.Windows.Forms.SystemInformation]::VirtualScreen.Height
    $pts = @(,@(20,20)) + @(,@(($vsW-20),20)) + @(,@(20,($vsH-20))) + @(,@(($vsW-20),($vsH-20)))
    $cornerHits = 0
    foreach ($pt in $pts) {
        if (Test-OverlayColor ([Pix]::GetPixel($dc, $pt[0], $pt[1]))) { $cornerHits++ }
        $L += ('PIXEL corner({0},{1}) RGB check done' -f $pt[0], $pt[1])
    }
    [Pix]::ReleaseDC([IntPtr]::Zero, $dc) | Out-Null

    $L += "PIXEL gui-area samples=$total overlay-colored=$hits"
    $L += "PIXEL corners showing overlay color: $cornerHits/4 (expected 4/4 while fixture covers desktop)"
    $pixOk = $hits -lt ($total * 0.5)
    $verdictWord = 'FAIL'
    if ($zOk -and $pixOk) { $verdictWord = 'GUI VISIBLE ON TOP OF OVERLAY' }
    $L += "VERDICT: zorder=$zOk pixels=$pixOk => $verdictWord"
    return @{ Ok = ($zOk -and $pixOk); Log = $L; GuiRect = $gui.Rect }
}


# ---------------------------------------------------------------------------
if ($Phase -eq 'Setup') {
    $L = @("=== SETUP $(Get-Date -Format o) user=$env:USERNAME ===")

    # WebView2 runtime check (required by cure-gui)
    $pv = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv
    if (-not $pv) { $pv = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv }
    if ($pv) { $L += "WebView2 runtime: PRESENT ($pv)" }
    else {
        $L += 'WebView2 runtime: MISSING. Install it first (MicrosoftEdgeWebview2Setup.exe from Microsoft, or install Edge), then re-run Setup.'
        Save-Evidence '01-setup-aborted' ($L -join "`r`n")
        Write-Host ($L -join "`n")
        exit 1
    }

    # Clean-slate checks
    $pre = @(Get-Process cure-watch -ErrorAction SilentlyContinue)
    $L += "pre-existing cure-watch processes: $($pre.Count) (expected 0)"
    if ($pre.Count -gt 0) { $pre | Stop-Process -Force; $L += '(killed pre-existing instances)' }

    # Copy payload from stage with integrity hashes
    $hashes = @{}
    foreach ($exe in 'cure-watch.exe','cure-gui.exe','fake-overlay.exe') {
        Copy-Item "$Stage\$exe" "$GuestDir\$exe" -Force
        $hashes[$exe] = (Get-FileHash "$GuestDir\$exe" -Algorithm SHA256).Hash
        $L += ('copied {0} sha256={1}' -f $exe, $hashes[$exe])
    }

    # Archive any old watcher log so later phases parse only fresh entries
    if (Test-Path $WatchLog) {
        Move-Item $WatchLog "$WatchLog.pre-validation-$Stamp" -Force
        $L += "archived old log -> cure-watch.log.pre-validation-$Stamp"
    }

    # Step 3: manual run ONCE
    $p = Start-Process -FilePath (Join-Path $GuestDir 'cure-watch.exe') -WorkingDirectory $GuestDir -PassThru
    Start-Sleep -Seconds 5

    try   { $p.Refresh(); $stillAlive = -not $p.HasExited } catch { $stillAlive = $false }
    $installLine = Select-String -Path $WatchLog -Pattern '\[install\]' -ErrorAction SilentlyContinue
    $startupLine = Select-String -Path $WatchLog -Pattern '\[startup\]' -ErrorAction SilentlyContinue
    $startupExists = Test-Path $StartupExe
    $hashMatch = $false
    if ($startupExists) { $hashMatch = ((Get-FileHash $StartupExe -Algorithm SHA256).Hash -eq $hashes['cure-watch.exe']) }

    $L += "manual run pid=$($p.Id) alive=$stillAlive"
    $L += "watcher log exists: $(Test-Path $WatchLog)"
    $L += "[install] line present: $([bool]$installLine)"
    if ($installLine) { $L += "  -> $($installLine.Line)" }
    $L += "[startup] line present: $([bool]$startupLine)"
    if ($startupLine) { $L += "  -> $($startupLine.Line)" }
    $L += "startup-folder file exists: $startupExists"
    $L += "startup-folder copy sha256 matches source: $hashMatch"

    $setupPass = $stillAlive -and [bool]$installLine -and [bool]$startupLine -and $startupExists -and $hashMatch
    $L += "SETUP VERDICT: $(if ($setupPass) {'PASS'} else {'FAIL'})"

    # Step 4: kill the manually-launched instance; from here on only the
    # Startup-folder copy may ever run again.
    if ($stillAlive) { Stop-Process -Id $p.Id -Force; Start-Sleep -Seconds 2 }
    $afterKill = @(Get-Process cure-watch -ErrorAction SilentlyContinue).Count
    $L += "after taskkill, cure-watch processes: $afterKill (expected 0)"

    $state = @{
        manual_pid          = $p.Id
        setup_completed_utc = [DateTime]::UtcNow.ToString('o')
        source_sha256       = $hashes['cure-watch.exe']
    }
    $state | ConvertTo-Json | Set-Content $StateFile -Encoding UTF8
    $L += "state saved: $StateFile (manual_pid=$($p.Id))"

    Save-Evidence '01-setup' ($L -join "`r`n")
    Write-Host ""
    Write-Host "================ SUMMARY ================"
    Write-Host ($L -join "`n")
    Write-Host ""
    Write-Host "NEXT: reboot the VM (Start > Restart), log back in, then run:"
    Write-Host "  powershell -NoProfile -ExecutionPolicy Bypass -File $GuestDir\run-cure-validation.ps1 -Phase PostReboot"
    exit $(if ($setupPass) { 0 } else { 1 })
}


# ---------------------------------------------------------------------------
if ($Phase -eq 'PostReboot') {
    $L = @("=== POST-REBOOT CHECK $(Get-Date -Format o) user=$env:USERNAME ===")
    if (-not (Test-Path $StateFile)) {
        $L += "FATAL: $StateFile missing - run the Setup phase first (pre-reboot)."
        Save-Evidence '02-postreboot-aborted' ($L -join "`r`n")
        Write-Host ($L -join "`n"); exit 1
    }
    $state = Get-Content $StateFile -Raw | ConvertFrom-Json
    $setupEnd = [datetime]::Parse($state.setup_completed_utc, [Globalization.CultureInfo]::InvariantCulture, [System.Globalization.DateTimeStyles]::RoundtripKind)
    $minsAgo = [math]::Round(([DateTime]::UtcNow - $setupEnd.ToUniversalTime()).TotalMinutes, 1)
    $L += "manual-run pid was $($state.manual_pid); setup completed $minsAgo minutes ago"

    $procs = @(Get-Process cure-watch -ErrorAction SilentlyContinue)
    $L += "cure-watch processes now running: $($procs.Count)"
    if ($procs.Count -ne 1) {
        $L += "POST-REBOOT VERDICT: FAIL (expected exactly 1 auto-launched instance, found $($procs.Count))"
        Save-Evidence '02-postreboot' ($L -join "`r`n")
        Write-Host ($L -join "`n"); exit 1
    }
    $proc = $procs[0]
    $procPath = ''
    try { $procPath = $proc.Path } catch {}
    $L += "live pid=$($proc.Id) path='$procPath'"
    $isStartupCopy = ($procPath.Trim().ToLower() -eq $StartupExe.Trim().ToLower())
    $L += "running binary IS the Startup-folder copy: $isStartupCopy"
    $differsFromManual = ($proc.Id -ne [int]$state.manual_pid)
    $L += "pid differs from manual-run pid: $differsFromManual"

    $entries = @(Parse-StartupEntries | Where-Object { $_.Pid -eq $proc.Id })
    $freshEntry = $entries | Where-Object { $_.Utc -gt $setupEnd.ToUniversalTime() } | Select-Object -First 1
    $L += "fresh [startup] log entry after setup completed: $([bool]$freshEntry)"
    if ($freshEntry) {
        $ageMin = [math]::Round(([DateTime]::UtcNow - $freshEntry.Utc).TotalMinutes, 1)
        $L += "  -> $($freshEntry.Raw)"
        $L += "  -> logged $ageMin min ago"
    }
    $L += "--- last 10 watcher log lines ---"
    $L += (Get-Content $WatchLog -Tail 10 -ErrorAction SilentlyContinue)

    $pass = $isStartupCopy -and $differsFromManual -and [bool]$freshEntry
    if ($pass) { $v = 'PASS - self-installed copy launched itself at login with zero manual action' }
    else       { $v = 'FAIL - see lines above' }
    $L += "POST-REBOOT VERDICT: $v"
    Save-Evidence '02-postreboot' ($L -join "`r`n")
    Write-Host ""
    Write-Host "================ SUMMARY ================"
    Write-Host ($L -join "`n")
    if ($pass) {
        Write-Host ""
        Write-Host "NEXT: run the OverlayAndUSB phase (tell the host operator you are ready for USB attach):"
        Write-Host "  powershell -NoProfile -ExecutionPolicy Bypass -File $GuestDir\run-cure-validation.ps1 -Phase OverlayAndUSB"
    }
    exit $(if ($pass) { 0 } else { 1 })
}


# ---------------------------------------------------------------------------
if ($Phase -eq 'OverlayAndUSB') {
    $L = @("=== OVERLAY+USB PHASE $(Get-Date -Format o) ===")

    $watcher = @(Get-Process cure-watch -ErrorAction SilentlyContinue)
    if ($watcher.Count -eq 0) {
        $L += 'FATAL: cure-watch not running - run the PostReboot phase first.'
        Save-Evidence '03-overlay-usb-aborted' ($L -join "`r`n")
        Write-Host ($L -join "`n"); exit 1
    }
    $L += "watcher running pid=$($watcher[0].Id) path='$($watcher[0].Path)'"

    $beforeDrives = @(Get-CimInstance Win32_LogicalDisk | Select-Object -ExpandProperty DeviceID)
    $L += "drives before attach: $($beforeDrives -join ' ')"

    # Launch the fullscreen topmost test fixture
    Start-Process -FilePath (Join-Path $GuestDir 'fake-overlay.exe')
    Start-Sleep -Seconds 4
    $ovlRows = Get-ZRows 'TEST FIXTURE'
    $ovl = $ovlRows | Where-Object { $_.Vis } | Select-Object -First 1
    if (-not $ovl) {
        $L += 'FATAL: overlay window not visible.'
        Save-Evidence '03-overlay-usb-aborted' ($L -join "`r`n")
        Write-Host ($L -join "`n"); exit 1
    }
    Add-Type -AssemblyName System.Windows.Forms
    $vsW = [System.Windows.Forms.SystemInformation]::VirtualScreen.Width
    $vsH = [System.Windows.Forms.SystemInformation]::VirtualScreen.Height
    $coverPct = [math]::Round(100.0 * $ovl.W * $ovl.H / ([double]$vsW * [double]$vsH), 1)
    $L += "overlay: Z=$($ovl.Z) topmost=$($ovl.Topmost) rect=$($ovl.Rect) covers=$coverPct% of screen"
    Save-Screenshot 'a-overlay-covering'

    Write-Host ""
    Write-Host ">>> NOW TELL THE HOST OPERATOR TO ATTACH THE USB DRIVE TO THIS VM <<<"
    Write-Host "Polling for a new drive letter (up to 6 minutes)..."
    $newDrives = @()
    for ($i = 0; $i -lt 180; $i++) {
        Start-Sleep -Seconds 2
        $now = @(Get-CimInstance Win32_LogicalDisk | Select-Object -ExpandProperty DeviceID)
        $newDrives = @($now | Where-Object { $beforeDrives -notcontains $_ })
        if ($newDrives.Count -gt 0) { break }
    }
    if ($newDrives.Count -eq 0) {
        $L += 'FAIL: no new drive appeared within timeout.'
        Save-Evidence '03-overlay-usb-aborted' ($L -join "`r`n")
        Write-Host ($L -join "`n"); exit 1
    }
    $waitedSecs = $i * 2
    $L += "new drive(s) detected inside guest after ~${waitedSecs}s: $($newDrives -join ' ')"

    Write-Host "Drive attached. Waiting 30s for watcher poll + GUI launch + scan..."
    Start-Sleep -Seconds 30

    $L += "--- watcher log tail ---"
    $L += (Get-Content $WatchLog -Tail 25 -ErrorAction SilentlyContinue)

    $guiProc = Get-Process cure-gui -ErrorAction SilentlyContinue
    if ($guiProc) { $L += "cure-gui process: pid=$($guiProc.Id) started=$($guiProc.StartTime)" }
    else          { $L += "cure-gui process: NOT RUNNING" }

    $res = Test-GuiAboveOverlay
    $L += $res.Log
    Save-Screenshot 'b-after-launch'

    foreach ($d in $newDrives) {
        $root = "$d\"
        $trig = Get-Content (Join-Path $root '.cure-trigger') -Raw -ErrorAction SilentlyContinue
        if ($null -ne $trig) { $L += "drive $d .cure-trigger content: '$($trig.Trim())'" }
        else                 { $L += "drive $d .cure-trigger: MISSING/unreadable" }
        $bl = Get-Item (Join-Path $root 'baseline.json') -ErrorAction SilentlyContinue
        if ($bl) {
            $blHash = (Get-FileHash $bl.FullName -Algorithm SHA256).Hash
            $L += "drive $d baseline.json: PRESENT size=$($bl.Length)B written=$($bl.LastWriteTime) sha256=$blHash"
        } else {
            $L += "drive $d baseline.json: MISSING"
        }
    }

    $logText = (Get-Content $WatchLog -Raw -ErrorAction SilentlyContinue)
    $detected = $logText -match '\[drive\] new drive appeared'
    $validTrig = $logText -match '\[trigger\] VALID'
    $launched = $logText -match '\[launch\] launched'
    $guiOk = [bool]$guiProc
    $above = $res.Ok
    $baselineOk = (@($newDrives | Where-Object { Test-Path (Join-Path "$_\" 'baseline.json') }).Count -gt 0)

    $allPass = $detected -and $validTrig -and $launched -and $guiOk -and $above -and $baselineOk
    $L += "--- FINAL CHECKLIST ---"
    $L += "drive-detected-in-log:   $detected"
    $L += "trigger-valid-in-log:    $validTrig"
    $L += "gui-launched-in-log:     $launched"
    $L += "gui-process-running:     $guiOk"
    $L += "gui-visible-above-fix:   $above"
    $L += "baseline.json-on-drive:  $baselineOk"
    if ($allPass) { $v = 'PASS - full chain works on top of a real reboot, from the Startup-folder copy, above a fullscreen overlay' }
    else          { $v = 'FAIL - see checklist' }
    $L += "OVERLAY+USB VERDICT: $v"

    Save-Evidence '03-overlay-usb' ($L -join "`r`n")
    Write-Host ""
    Write-Host "================ SUMMARY ================"
    Write-Host ($L -join "`n")
    Write-Host ""
    Write-Host "Leave windows open for eyeballing. Cleanup happens via host-side snapshot restore."
    exit $(if ($allPass) { 0 } else { 1 })
}

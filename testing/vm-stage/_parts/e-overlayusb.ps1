
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

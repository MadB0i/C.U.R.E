
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


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

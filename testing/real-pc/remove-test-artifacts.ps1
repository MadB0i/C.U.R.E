#Requires -Version 5.1
<#
.SYNOPSIS
  Removes exactly the CURE_TEST_* artifacts created by make-test-artifacts.ps1
  and verifies each is gone. Prints a final clean/leftovers report.

.DESCRIPTION
  Removes, and nothing else:
    - %TEMP%\CURE_TEST\CURE_TEST_helper.exe
    - %TEMP%\CURE_TEST\CURE_TEST_file.txt (+ acl-before.txt, hash-before.txt)
    - %TEMP%\CURE_TEST\CURE_TEST_overlay.exe (dummy window binary, if built)
    - HKCU Run value CURE_TEST_run
    - Startup shortcut CURE_TEST_startup.lnk
    - Scheduled task CURE_TEST_task (attempted; absent = clean)
  The %TEMP%\CURE_TEST directory itself is removed only when empty afterwards.
  Watcher host files (cure-watch consent/marker/pairing/log, Startup copy)
  are NOT touched here — see README-TESTING.md step 9 for those exact paths.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File remove-test-artifacts.ps1 -DryRun
  powershell -NoProfile -ExecutionPolicy Bypass -File remove-test-artifacts.ps1
#>
[CmdletBinding()]
param(
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Prefix   = 'CURE_TEST_'
$TestDir  = Join-Path $env:TEMP 'CURE_TEST'
$RunKey   = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$RunValue = 'CURE_TEST_run'
$LnkPath  = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\CURE_TEST_startup.lnk'
$TaskName = 'CURE_TEST_task'

$script:leftovers = @()

function Assert-TestPath([string]$Path, [string]$Kind) {
    # Every filesystem target must live under %TEMP%\CURE_TEST or be the one
    # known Startup .lnk; every named item must carry the CURE_TEST_ prefix.
    $file = Split-Path $Path -Leaf
    if (-not $file.StartsWith($Prefix)) {
        throw "REFUSAL ($Kind): '$Path' is outside the CURE_TEST_ namespace. Aborting."
    }
    if ($Kind -eq 'testdir-file' -and (Split-Path $Path -Parent) -ne $TestDir) {
        throw "REFUSAL ($Kind): '$Path' is outside $TestDir. Aborting."
    }
}

function Remove-TestFile([string]$Path) {
    Assert-TestPath $Path 'testdir-file'
    if ($DryRun) { Write-Host "[DRY-RUN] would remove file: $Path"; return }
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Force
        Write-Host "removed file: $Path"
    } else {
        Write-Host "absent (already clean): $Path"
    }
    if (Test-Path -LiteralPath $Path) { $script:leftovers += "file still present: $Path" }
}

Write-Host "CURE real-PC artifact removal (verifies every item)"
Write-Host "DryRun: $DryRun"
Write-Host ""

# 1. Test-dir files ---------------------------------------------------------
foreach ($name in @('CURE_TEST_helper.exe', 'CURE_TEST_file.txt',
                    'CURE_TEST_acl-before.txt', 'CURE_TEST_hash-before.txt',
                    'CURE_TEST_overlay.exe')) {
    Remove-TestFile (Join-Path $TestDir $name)
}

# 2. HKCU Run value -----------------------------------------------------------
Write-Host ""
if ($DryRun) {
    Write-Host "[DRY-RUN] would remove registry value: $RunKey\$RunValue"
} else {
    $prop = Get-ItemProperty -LiteralPath $RunKey -Name $RunValue -ErrorAction SilentlyContinue
    if ($null -ne $prop) {
        Remove-ItemProperty -LiteralPath $RunKey -Name $RunValue -Force
        Write-Host "removed registry value: $RunKey\$RunValue"
    } else {
        Write-Host "absent (already clean): $RunKey\$RunValue"
    }
    if ($null -ne (Get-ItemProperty -LiteralPath $RunKey -Name $RunValue -ErrorAction SilentlyContinue)) {
        $script:leftovers += "registry value still present: $RunKey\$RunValue"
    }
}

# 3. Startup shortcut -----------------------------------------------------------
Write-Host ""
if ($LnkPath -notlike '*CURE_TEST_*') { throw "REFUSAL: unexpected lnk path: $LnkPath" }
if ($DryRun) {
    Write-Host "[DRY-RUN] would remove shortcut: $LnkPath"
} else {
    if (Test-Path -LiteralPath $LnkPath) {
        Remove-Item -LiteralPath $LnkPath -Force
        Write-Host "removed shortcut: $LnkPath"
    } else {
        Write-Host "absent (already clean): $LnkPath"
    }
    if (Test-Path -LiteralPath $LnkPath) { $script:leftovers += "shortcut still present: $LnkPath" }
}

# 4. Scheduled task ---------------------------------------------------------------
# NOTE: schtasks writes failures to stderr, which throws under
# $ErrorActionPreference='Stop' before $LASTEXITCODE is readable — hence
# the try/catch (a "not found" query is the expected absent case, not an
# error).
function Test-TaskPresent([string]$Name) {
    try {
        schtasks /Query /TN "\$Name" 2>$null | Out-Null
        return ($LASTEXITCODE -eq 0)
    } catch {
        return $false
    }
}
Write-Host ""
if ($DryRun) {
    Write-Host "[DRY-RUN] would run: schtasks /Delete /TN $TaskName /F"
    Write-Host "[DRY-RUN] would run: schtasks /Query /TN $TaskName (expect 'not exist')"
} else {
    if (Test-TaskPresent $TaskName) {
        schtasks /Delete /TN "\$TaskName" /F
        Write-Host "deleted scheduled task: \$TaskName"
    } else {
        Write-Host "absent (never created or already deleted): \$TaskName"
    }
    if (Test-TaskPresent $TaskName) { $script:leftovers += "scheduled task still present: \$TaskName" }
}

# 5. CURE data dir (baseline/quarantine the operator created via --data-dir) --
# Exact-name exception to the prefix rule: leaf must be exactly `cure-data`
# directly under $TestDir (the path README step 0 mandates). Anything else
# is left for the operator.
$DataDir = Join-Path $TestDir 'cure-data'
Write-Host ""
if ($DryRun) {
    Write-Host "[DRY-RUN] would remove $DataDir recursively (exact-name match only)"
} else {
    if ((Test-Path -LiteralPath $DataDir) -and (Split-Path $DataDir -Leaf) -eq 'cure-data') {
        Remove-Item -LiteralPath $DataDir -Recurse -Force
        Write-Host "removed data dir: $DataDir"
    } else {
        Write-Host "absent (already clean): $DataDir"
    }
    if (Test-Path -LiteralPath $DataDir) { $script:leftovers += "data dir still present: $DataDir" }
}

# 6. Test dir itself (only if empty) ----------------------------------------------
Write-Host ""
if ($DryRun) {
    Write-Host "[DRY-RUN] would remove $TestDir if empty afterwards"
} else {
    if ((Test-Path -LiteralPath $TestDir) -and @(Get-ChildItem -LiteralPath $TestDir -Force).Count -eq 0) {
        Remove-Item -LiteralPath $TestDir -Force
        Write-Host "removed empty dir: $TestDir"
    } elseif (Test-Path -LiteralPath $TestDir) {
        Write-Host "kept non-empty dir for inspection: $TestDir"
        $script:leftovers += "test dir not empty: $TestDir"
    } else {
        Write-Host "absent (already clean): $TestDir"
    }
}

# Final report ----------------------------------------------------------------------
Write-Host ""
if ($DryRun) {
    Write-Host "DRY-RUN done: plan printed above; nothing was changed or verified."
    exit 0
}
if ($script:leftovers.Count -eq 0) {
    Write-Host "RESULT: clean — all CURE_TEST_ artifacts verified gone."
    exit 0
} else {
    Write-Host "RESULT: LEFTOVERS — $($script:leftovers.Count) item(s) need attention:"
    foreach ($l in $script:leftovers) { Write-Host "  - $l" }
    exit 1
}

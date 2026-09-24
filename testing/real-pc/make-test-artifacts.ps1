#Requires -Version 5.1
<#
.SYNOPSIS
  Creates harmless CURE_TEST_* detection artifacts for real-PC C.U.R.E validation.
  Runs as a NORMAL user: HKCU and user-profile paths only. No admin required,
  nothing outside the CURE_TEST_ namespace is touched.

.DESCRIPTION
  Creates, under %TEMP%\CURE_TEST\ and the current user's own locations:
    1. CURE_TEST_helper.exe  - byte copy of notepad.exe (signed MS binary, inert)
    2. HKCU Run value        - CURE_TEST_run -> "<helper>" (registry-run source)
    3. Startup shortcut      - CURE_TEST_startup.lnk -> helper (startup-folder source)
    4. Per-user logon task   - CURE_TEST_task running helper (attempted; SKIPs
                               honestly if the Task Scheduler denies a standard
                               user, which it normally does for the root folder)
    5. CURE_TEST_file.txt    - with one explicit non-default ACE (Everyone:R),
                               plus saved icacls + SHA256 snapshots for the
                               quarantine/undo ACL-fidelity check.

  Every action prints what it does. -DryRun prints the plan and changes nothing.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File make-test-artifacts.ps1 -DryRun
  powershell -NoProfile -ExecutionPolicy Bypass -File make-test-artifacts.ps1
#>
[CmdletBinding()]
param(
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Prefix      = 'CURE_TEST_'
$TestDir     = Join-Path $env:TEMP 'CURE_TEST'
$HelperName  = 'CURE_TEST_helper.exe'
$HelperPath  = Join-Path $TestDir $HelperName
$AclFileName = 'CURE_TEST_file.txt'
$AclFilePath = Join-Path $TestDir $AclFileName
$RunKey      = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$RunValue    = 'CURE_TEST_run'
$LnkName     = 'CURE_TEST_startup.lnk'
$TaskName    = 'CURE_TEST_task'
$Notepad     = Join-Path $env:SystemRoot 'System32\notepad.exe'

function Assert-TestName([string]$Name) {
    # Hard guard: this kit may only ever create names in its own namespace.
    if (-not $Name.StartsWith($Prefix)) {
        throw "REFUSAL: '$Name' does not start with '$Prefix'. Aborting."
    }
}

function Invoke-Step([string]$Label, [string]$DryText, [scriptblock]$Action) {
    Write-Host ""
    Write-Host "--- $Label"
    if ($DryRun) {
        Write-Host "[DRY-RUN] would: $DryText"
        return $null
    }
    Write-Host "do: $DryText"
    return & $Action
}

function Test-IsElevated {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p  = New-Object Security.Principal.WindowsPrincipal($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

Write-Host "CURE real-PC test artifact setup (normal user, HKCU/user-profile only)"
Write-Host "Test dir : $TestDir"
Write-Host "DryRun   : $DryRun"
if (Test-IsElevated) {
    Write-Host "WARNING: this shell IS elevated. The kit is designed for a normal"
    Write-Host "user; results (especially the scheduled-task step) may differ."
} else {
    Write-Host "Elevation: standard user (as intended)."
}

foreach ($n in @($HelperName, $AclFileName, $RunValue, $LnkName, $TaskName)) {
    Assert-TestName $n
}
if (-not (Test-Path $Notepad)) { throw "prerequisite missing: $Notepad" }

# 1. Test directory -------------------------------------------------------
Invoke-Step "test directory" "create $TestDir" {
    New-Item -ItemType Directory -Path $TestDir -Force | Out-Null
    Write-Host "created: $TestDir"
}

# 2. Harmless test binary (byte copy of signed notepad) --------------------
Invoke-Step "test binary" "copy $Notepad -> $HelperPath" {
    Copy-Item -LiteralPath $Notepad -Destination $HelperPath -Force
    $h = (Get-FileHash -LiteralPath $HelperPath -Algorithm SHA256).Hash
    Write-Host "copied : $HelperPath"
    Write-Host "sha256 : $h"
    Write-Host "note   : bytes are Microsoft-signed notepad; CURE should DETECT"
    Write-Host "         (not necessarily flag) anything pointing at it."
}

# 3. HKCU Run value (registry-run source; never quarantinable by design) ---
Invoke-Step "HKCU Run value" "set $RunKey\$RunValue = `"$HelperPath`"" {
    New-ItemProperty -LiteralPath $RunKey -Name $RunValue `
        -Value "`"$HelperPath`"" -PropertyType String -Force | Out-Null
    Write-Host "set    : $RunKey\$RunValue"
}

# 4. Startup-folder shortcut (startup-folder source; quarantinable) --------
$StartupDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup'
$LnkPath = Join-Path $StartupDir $LnkName
Invoke-Step "Startup shortcut" "create $LnkPath -> $HelperPath" {
    $sh = New-Object -ComObject WScript.Shell
    try {
        $sc = $sh.CreateShortcut($LnkPath)
        $sc.TargetPath = $HelperPath
        $sc.Description = 'CURE real-PC test artifact (harmless)'
        $sc.Save()
    } finally {
        [Runtime.InteropServices.Marshal]::ReleaseComObject($sh) | Out-Null
    }
    Write-Host "created: $LnkPath"
}

# 5. Per-user logon task (attempted; standard users are usually denied) ----
Invoke-Step "scheduled task" "schtasks /Create /TN $TaskName /TR helper /SC ONLOGON /F" {
    $out = schtasks /Create /TN $TaskName /TR "`"$HelperPath`"" /SC ONLOGON /F 2>&1
    $code = $LASTEXITCODE
    Write-Host ($out -join "`n")
    if ($code -ne 0) {
        Write-Host "SKIP   : task creation refused (exit $code) — expected for a"
        Write-Host "         standard user; the Task Scheduler root folder needs"
        Write-Host "         elevation. Checklist step 2b covers this case."
    } else {
        Write-Host "created: scheduled task \$TaskName"
    }
}

# 6. ACL fidelity file + snapshots -----------------------------------------
Invoke-Step "ACL test file" "create $AclFilePath with explicit Everyone:R ACE + snapshots" {
    Set-Content -LiteralPath $AclFilePath -Value 'CURE real-PC ACL fidelity probe (harmless).' -NoNewline
    icacls $AclFilePath /grant '*S-1-1-0:(R)' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls grant failed on $AclFilePath" }
    $aclOut = Join-Path $TestDir 'CURE_TEST_acl-before.txt'
    icacls $AclFilePath > $aclOut
    $hashOut = Join-Path $TestDir 'CURE_TEST_hash-before.txt'
    (Get-FileHash -LiteralPath $AclFilePath -Algorithm SHA256).Hash > $hashOut
    Write-Host "created: $AclFilePath (explicit Everyone:R ACE)"
    Write-Host "saved  : $aclOut"
    Write-Host "saved  : $hashOut"
    Write-Host "note   : quarantine this file via 'cure --startup-root $TestDir',"
    Write-Host "         then 'cure undo <id>' and diff icacls output (ACE lines must"
    Write-Host "         match; control-flag-only diffs are acceptable per core/src/acl.rs)."
}

Write-Host ""
Write-Host "=== setup done $(if ($DryRun) { '(DRY-RUN: nothing was changed)' }) ==="
Write-Host "CURE should now detect: HKCU Run value '$RunValue', shortcut '$LnkName',"
Write-Host "and (if task creation succeeded) scheduled task '$TaskName'."
Write-Host "Remove everything with: remove-test-artifacts.ps1"

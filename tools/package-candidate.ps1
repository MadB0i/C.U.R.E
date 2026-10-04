param(
    [string]$Version = '0.2.0',
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repo "release-candidate/v$Version" }
$destination = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $destination) { throw "Output already exists: $destination. Choose a new directory; existing candidates are never overwritten." }
$candidateSha = git -C $repo rev-parse HEAD
if ($LASTEXITCODE) { throw 'Cannot resolve source commit' }
if (git -C $repo status --porcelain) { throw 'Commit tracked source changes before packaging a candidate.' }
& (Join-Path $PSScriptRoot 'build-release.ps1')
$stage = Join-Path $destination 'portable'
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'gui/src-tauri/target/release/cure-gui.exe') -Destination $stage
foreach ($name in @('cure.exe', 'cure-watch.exe')) {
    Copy-Item -LiteralPath (Join-Path $repo "target/release/$name") -Destination $stage
}
foreach ($name in @('README.md', 'LICENSE', 'SECURITY.md')) {
    Copy-Item -LiteralPath (Join-Path $repo $name) -Destination $stage
}
Copy-Item -LiteralPath (Join-Path $repo "docs/RELEASE-NOTES-$Version.md") -Destination (Join-Path $stage 'RELEASE-NOTES.md')
New-Item -ItemType Directory -Path (Join-Path $stage 'docs/media') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'docs/screenshots') -Destination (Join-Path $stage 'docs') -Recurse
Copy-Item -LiteralPath (Join-Path $repo 'docs/media/demo.gif') -Destination (Join-Path $stage 'docs/media')
@("Version: $Version", "Source commit: $candidateSha", 'Unsigned local candidate; not a published release.') |
    Set-Content -LiteralPath (Join-Path $stage 'BUILD-INFO.txt') -Encoding ascii
$binarySums = Get-ChildItem -LiteralPath $stage -Filter '*.exe' | Sort-Object Name | ForEach-Object {
    "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)"
}
$binarySums | Set-Content -LiteralPath (Join-Path $stage 'SHA256SUMS.txt') -Encoding ascii
$zipPath = Join-Path $destination "cure-v$Version.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipPath
$allSums = @($binarySums) + "$((Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant())  cure-v$Version.zip"
$allSums | Set-Content -LiteralPath (Join-Path $destination 'SHA256SUMS.txt') -Encoding ascii
Write-Output "Prepared $zipPath from $candidateSha"

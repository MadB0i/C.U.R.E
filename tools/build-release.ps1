$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
try {
    # Panic/source locations must not disclose the builder's workspace/profile.
    # Encoded arguments also support paths containing spaces.
    $flags = @('-D', 'warnings', "--remap-path-prefix=$repo=/cure")
    if ($env:USERPROFILE) { $flags += "--remap-path-prefix=$env:USERPROFILE=/build-user" }
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1f
    cargo build --release --workspace --manifest-path (Join-Path $repo 'Cargo.toml')
    if ($LASTEXITCODE) { throw "Workspace release build failed: $LASTEXITCODE" }
    cargo build --release --manifest-path (Join-Path $repo 'gui/src-tauri/Cargo.toml')
    if ($LASTEXITCODE) { throw "GUI release build failed: $LASTEXITCODE" }
} finally {
    if ($null -eq $previousFlags) { Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue }
    else { $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags }
}

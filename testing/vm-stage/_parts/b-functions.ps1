
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

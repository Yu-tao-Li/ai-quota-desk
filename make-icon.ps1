Add-Type -AssemblyName System.Drawing
$size = 1024
$out = 'C:\Users\Administrator\.zcode\workspace\default\ai-quota-desk\app-icon.png'

$bmp = New-Object System.Drawing.Bitmap($size, $size)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias
$g.Clear([System.Drawing.Color]::Transparent)

# rounded square background, blue -> purple gradient
function New-RoundRect($x, $y, $w, $h, $r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}
$rect = New-RoundRect 32 32 ($size - 64) ($size - 64) 230
$brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Point(0, 0)), (New-Object System.Drawing.Point($size, $size)),
    [System.Drawing.Color]::FromArgb(255, 62, 110, 245),
    [System.Drawing.Color]::FromArgb(255, 139, 92, 246))
$g.FillPath($brush, $rect)

# gauge ring: translucent white track + white progress arc (75% of 270 deg)
$cx = $size / 2; $cy = $size / 2; $r = 300

$trackPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(70, 255, 255, 255), 78)
$trackPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$trackPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$g.DrawArc($trackPen, [int]($cx - $r), [int]($cy - $r), [int](2 * $r), [int](2 * $r), [float]135, [float]270)

$arcPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 255, 255, 255), 78)
$arcPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$arcPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$g.DrawArc($arcPen, [int]($cx - $r), [int]($cy - $r), [int](2 * $r), [int](2 * $r), [float]135, [float]202)

# center text AQ
$font = New-Object System.Drawing.Font('Segoe UI', 195, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$sf = New-Object System.Drawing.StringFormat
$sf.Alignment = [System.Drawing.StringAlignment]::Center
$sf.LineAlignment = [System.Drawing.StringAlignment]::Center
$g.DrawString('AQ', $font, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF(0, 0, $size, $size)), $sf)

$g.Dispose()
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved: $out"

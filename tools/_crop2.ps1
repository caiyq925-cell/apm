Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Image]::FromFile("D:\Users\ai_apm\apm-monitor\tools\_main_now.png")
$scale = 3
$srcRect = New-Object System.Drawing.Rectangle(0, 30, 700, 50)
$bmp = New-Object System.Drawing.Bitmap(($srcRect.Width * $scale), ($srcRect.Height * $scale))
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$dstRect = New-Object System.Drawing.Rectangle(0, 0, $bmp.Width, $bmp.Height)
$g.DrawImage($img, $dstRect, $srcRect, [System.Drawing.GraphicsUnit]::Pixel)
$g.Dispose()
$bmp.Save("D:\Users\ai_apm\apm-monitor\tools\_main_session.png", [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose(); $img.Dispose()
Write-Output "ok"

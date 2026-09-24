param([string]$Src, [string]$Out, $X, $Y, $W, $H, $Scale = 3)
$X = [int]$X; $Y = [int]$Y; $W = [int]$W; $H = [int]$H; $Scale = [int]$Scale
Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Image]::FromFile($Src)
$bmp = New-Object System.Drawing.Bitmap($W * $Scale, $H * $Scale)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.DrawImage($img, (New-Object System.Drawing.Rectangle(0, 0, ($W * $Scale), ($H * $Scale))), (New-Object System.Drawing.Rectangle($X, $Y, $W, $H)), [System.Drawing.GraphicsUnit]::Pixel)
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose(); $img.Dispose()
Write-Output "saved $Out"

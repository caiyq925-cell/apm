Add-Type -AssemblyName System.Windows.Forms, System.Drawing

# 打印虚拟屏幕信息
$vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
Write-Output "VirtualScreen: $($vs.X),$($vs.Y) $($vs.Width)x$($vs.Height)"

# 目标进程主窗口
$p = Get-Process -Id 36816 -ErrorAction SilentlyContinue
if ($p) { Write-Output "MainWindowTitle: $($p.MainWindowTitle)  HWND: $($p.MainWindowHandle)" }

$bmp = New-Object System.Drawing.Bitmap($vs.Width, $vs.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($vs.X, $vs.Y, 0, 0, $bmp.Size)
$out = "D:\Users\ai_apm\apm-monitor\tools\_screen.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "saved: $out"

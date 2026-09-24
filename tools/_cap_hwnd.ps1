param([long]$Hwnd, [string]$Out = "D:\Users\ai_apm\apm-monitor\tools\_win.png")
Add-Type -AssemblyName System.Drawing
$code = @"
using System;
using System.Runtime.InteropServices;
public class W2 {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -TypeDefinition $code
$h = [IntPtr]$Hwnd
$sb = New-Object System.Text.StringBuilder 512
[void][W2]::GetWindowText($h, $sb, 512)
$r = New-Object W2+RECT
[void][W2]::GetWindowRect($h, [ref]$r)
$w = $r.R - $r.L; $hh = $r.B - $r.T
Write-Output ("title='" + $sb.ToString() + "' size=" + $w + "x" + $hh + " vis=" + [W2]::IsWindowVisible($h))
if ($w -lt 50 -or $hh -lt 50) { Write-Output "window too small, forcing min 1080x760 for capture"; $w = 1080; $hh = 760 }
$bmp = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [W2]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "PrintWindow=$ok saved=$Out"

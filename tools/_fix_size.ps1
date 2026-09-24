param([long]$Hwnd, [int]$W = 1100, [int]$H = 780, [string]$Out = "")
Add-Type -AssemblyName System.Drawing
$code = @"
using System;
using System.Runtime.InteropServices;
public class W4 {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -TypeDefinition $code
$h = [IntPtr]$Hwnd
# SWP_NOZORDER=4, SWP_SHOWWINDOW=0x40 ; move to (60,60) and resize
[void][W4]::ShowWindow($h, 9)  # SW_RESTORE
[void][W4]::SetWindowPos($h, [IntPtr]::Zero, 60, 60, $W, $H, 0x0044)
Start-Sleep -Milliseconds 800
$r = New-Object W4+RECT
[void][W4]::GetWindowRect($h, [ref]$r)
$rw = $r.R - $r.L; $rh = $r.B - $r.T
Write-Output "after resize: ${rw}x${rh} at ($($r.L),$($r.T))"
if ($Out -ne "") {
  $bmp = New-Object System.Drawing.Bitmap($rw, $rh)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  $ok = [W4]::PrintWindow($h, $hdc, 2)
  $g.ReleaseHdc($hdc); $g.Dispose()
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  Write-Output "PrintWindow=$ok saved=$Out"
}

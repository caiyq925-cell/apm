param(
  [int]$Count = 150,
  [int]$Px = 60,
  [int]$Py = 250,
  [string]$Out = "D:\Users\ai_apm\apm-monitor\tools\_win_scroll.png",
  [int]$Top = 41030192,
  [int]$Gap = 250
)
Add-Type -AssemblyName System.Drawing

$code = @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
public class W3 {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public static List<IntPtr> Kids(IntPtr root) {
    var list = new List<IntPtr>();
    EnumChildWindows(root, (h, l) => { list.Add(h); return true; }, IntPtr.Zero);
    return list;
  }
  public static string Cls(IntPtr h) { var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString(); }
}
"@
Add-Type -TypeDefinition $code

$top = [IntPtr]$Top
$r = New-Object W3+RECT
[void][W3]::GetWindowRect($top, [ref]$r)

$widget = [IntPtr]::Zero
foreach ($k in [W3]::Kids($top)) {
  if ([W3]::Cls($k) -like "*RenderWidgetHost*") { $widget = $k; break }
}
if ($widget -eq [IntPtr]::Zero) { $widget = $top }

$x = $r.L + $Px
$y = $r.T + $Py
$lp = [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF))
Write-Output ("widget=" + $widget + " point=" + $x + "," + $y)
for ($i = 0; $i -lt $Count; $i++) {
  [void][W3]::PostMessage($widget, 0x020A, [IntPtr]([int](-120 * 65536)), $lp)
  Start-Sleep -Milliseconds $Gap
}
Start-Sleep -Milliseconds 700

$w = $r.R - $r.L; $h = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [W3]::PrintWindow($top, $hdc, 2)
$g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output ("PrintWindow=" + $ok + " saved=" + $Out)

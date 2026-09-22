Add-Type -AssemblyName System.Drawing

$code = @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -TypeDefinition $code

$target = 36816
$found = @()
$cb = [W+EnumProc]{
  param($h, $l)
  $pid2 = 0
  [void][W]::GetWindowThreadProcessId($h, [ref]$pid2)
  if ($pid2 -eq $target) {
    $sb = New-Object System.Text.StringBuilder 512
    [void][W]::GetWindowText($h, $sb, 512)
    $r = New-Object W+RECT
    [void][W]::GetWindowRect($h, [ref]$r)
    $script:found += [pscustomobject]@{
      HWND = $h; Visible = [W]::IsWindowVisible($h); Title = $sb.ToString()
      W = $r.R - $r.L; H = $r.B - $r.T
    }
  }
  return $true
}
[void][W]::EnumWindows($cb, [IntPtr]::Zero)
$found | Format-Table -AutoSize | Out-String -Width 200 | Write-Output

# 选面积最大的可见窗口
$win = $found | Where-Object { $_.Visible -and $_.W -gt 200 -and $_.H -gt 200 } | Sort-Object { $_.W * $_.H } -Descending | Select-Object -First 1
if (-not $win) { Write-Output "no visible window"; exit 1 }
Write-Output "capturing HWND=$($win.HWND) $($win.W)x$($win.H)"

$bmp = New-Object System.Drawing.Bitmap($win.W, $win.H)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [W]::PrintWindow($win.HWND, $hdc, 2)   # PW_RENDERFULLCONTENT
$g.ReleaseHdc($hdc)
$g.Dispose()
$out = "D:\Users\ai_apm\apm-monitor\tools\_win.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "PrintWindow=$ok saved=$out"

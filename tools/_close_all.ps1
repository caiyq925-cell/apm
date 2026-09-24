# Gracefully close all apm-monitor windows via WM_CLOSE (never taskkill /F).
$code = @"
using System;
using System.Runtime.InteropServices;
public class W3 {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
Add-Type -TypeDefinition $code
$pids = (Get-Process apm-monitor -ErrorAction SilentlyContinue).Id
if (-not $pids) { Write-Output "no apm-monitor processes"; exit 0 }
foreach ($target in $pids) {
  $cb = [W3+EnumProc]{
    param($h, $l)
    $pid2 = 0
    [void][W3]::GetWindowThreadProcessId($h, [ref]$pid2)
    if ($pid2 -eq $target) { [void][W3]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
    return $true
  }
  [void][W3]::EnumWindows($cb, [IntPtr]::Zero)
  Write-Output "WM_CLOSE posted to all windows of PID $target"
}

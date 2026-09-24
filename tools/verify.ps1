param(
  [int]$ProcId,
  [string]$Out = "D:\Users\ai_apm\apm-monitor\tools\_verify.txt"
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$startTxt = [string]::Join('', [char[]](0x5F00, 0x59CB, 0x7EDF, 0x8BA1))   # kai shi tong ji
$logTitle = [string]::Join('', [char[]](0x5B9E, 0x65F6, 0x65E5, 0x5FD7))   # shi shi ri zhi

$code = @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
public class W5 {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public static IntPtr Main(uint want) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == want && IsWindowVisible(h)) {
        RECT r; GetWindowRect(h, out r);
        if (r.R - r.L > 400 && r.B - r.T > 300) { found = h; return false; }
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
Add-Type -TypeDefinition $code

$hwnd = [W5]::Main([uint32]$ProcId)
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no main window for pid $ProcId"; exit 1 }
Write-Output ("hwnd=" + $hwnd)

$root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)

function Dump($el, $sb, $depth, $maxDepth) {
  if ($depth -gt $maxDepth) { return }
  $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
  $child = $walker.GetFirstChild($el)
  while ($child -ne $null) {
    $c = $child.Current
    $exp = ""
    try {
      $p = $null
      if ($child.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$p)) {
        $exp = " ExpandCollapse=" + $p.Current.ExpandCollapseState
      }
    } catch {}
    [void]$sb.AppendLine(("  " * $depth) + "[" + $c.ControlType.ProgrammaticName.Replace("ControlType.", "") + "] name='" + $c.Name + "' id='" + $c.AutomationId + "' off=" + $c.IsOffscreen + $exp)
    Dump $child $sb ($depth + 1) $maxDepth
    $child = $walker.GetNextSibling($child)
  }
}

$sb0 = New-Object System.Text.StringBuilder
Dump $root $sb0 0 16
$before = $sb0.ToString()

$cond = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Button)
$btns = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
$clicked = $false
foreach ($b in $btns) {
  if ($b.Current.Name -eq $startTxt) {
    $ip = $null
    if ($b.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$ip)) {
      $ip.Invoke()
      $clicked = $true
      Write-Output ("invoked button '" + $b.Current.Name + "'")
      break
    }
  }
}
if (-not $clicked) { Write-Output "start button not found or not invokable" }

Start-Sleep -Seconds 12

$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine("===== BEFORE CLICK =====")
[void]$sb.Append($before)
[void]$sb.AppendLine("===== AFTER CLICK (12s) =====")
Dump $root $sb 0 16
[System.IO.File]::WriteAllText($Out, $sb.ToString(), [System.Text.Encoding]::UTF8)
Write-Output ("written " + $Out)

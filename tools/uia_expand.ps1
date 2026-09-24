param(
  [int]$Hwnd = 41030192,
  [string]$Out = "D:\Users\ai_apm\apm-monitor\tools\_uia2.txt"
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$logTitle = [string]::Join('', [char[]](0x5B9E, 0x65F6, 0x65E5, 0x5FD7))

$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
if ($root -eq $null) { Write-Output "no root"; exit 1 }

$cond = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Button)
$btns = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
Write-Output ("buttons=" + $btns.Count)

$target = $null
foreach ($b in $btns) {
  $n = $b.Current.Name
  if ($n -and $n.StartsWith($logTitle)) { $target = $b; break }
}
if ($target -eq $null) { Write-Output "log panel not found"; exit 1 }
Write-Output ("target name='" + $target.Current.Name + "'")

$p = $null
if ($target.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$p)) {
  Write-Output ("before=" + $p.Current.ExpandCollapseState)
  $p.Expand()
  Start-Sleep -Seconds 2
  Write-Output ("after=" + $p.Current.ExpandCollapseState)
} else {
  Write-Output "no ExpandCollapse pattern"
}

# 展开后再 dump 一次，重点看日志框文本
function Dump($el, $sb, $depth, $maxDepth) {
  if ($depth -gt $maxDepth) { return }
  $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
  $child = $walker.GetFirstChild($el)
  while ($child -ne $null) {
    $c = $child.Current
    [void]$sb.AppendLine(("  " * $depth) + "[" + $c.ControlType.ProgrammaticName.Replace("ControlType.", "") + "] name='" + $c.Name + "' id='" + $c.AutomationId + "' off=" + $c.IsOffscreen)
    Dump $child $sb ($depth + 1) $maxDepth
    $child = $walker.GetNextSibling($child)
  }
}
$sb = New-Object System.Text.StringBuilder
Dump $root $sb 0 16
[System.IO.File]::WriteAllText($Out, $sb.ToString(), [System.Text.Encoding]::UTF8)
Write-Output ("written " + $Out + " lines=" + ([System.IO.File]::ReadAllLines($Out).Count))

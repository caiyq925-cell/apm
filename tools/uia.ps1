param(
  [int]$Hwnd = 41030192,
  [string]$Out = "D:\Users\ai_apm\apm-monitor\tools\_uia.txt"
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$logTitle = [string]::Join('', [char[]](0x5B9E, 0x65F6, 0x65E5, 0x5FD7))   # "shi shi ri zhi"
$clearTxt = [string]::Join('', [char[]](0x6E05, 0x7A7A))                 # "qing kong"
$resTitle = [string]::Join('', [char[]](0x7EDF, 0x8BA1, 0x7ED3, 0x679C)) # "tong ji jie guo"

$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
if ($root -eq $null) { Write-Output "no root"; exit 1 }
Write-Output ("root: " + $root.Current.Name + " class=" + $root.Current.ClassName)

function Dump($el, $sb, $depth, $maxDepth) {
  if ($depth -gt $maxDepth) { return }
  $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
  $child = $walker.GetFirstChild($el)
  while ($child -ne $null) {
    $c = $child.Current
    $line = ("  " * $depth) + "[" + $c.ControlType.ProgrammaticName.Replace("ControlType.", "") + "] name='" + $c.Name + "' id='" + $c.AutomationId + "' cls='" + $c.ClassName + "' off=" + $c.IsOffscreen
    # patterns
    try {
      $p = $null
      if ($child.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$p)) {
        $line += " ExpandCollapse=" + $p.Current.ExpandCollapseState
      }
    } catch {}
    try {
      $p2 = $null
      if ($child.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$p2)) {
        $v = $p2.Current.Value
        if ($v) { $line += " Value='" + ($v -replace "`r?`n", " | ") + "'" }
      }
    } catch {}
    [void]$sb.AppendLine($line)
    Dump $child $sb ($depth + 1) $maxDepth
    $child = $walker.GetNextSibling($child)
  }
}

foreach ($pass in 1..2) {
  $sb = New-Object System.Text.StringBuilder
  [void]$sb.AppendLine("===== PASS $pass =====")
  Dump $root $sb 0 14
  [System.IO.File]::WriteAllText($Out, $sb.ToString(), [System.Text.Encoding]::UTF8)
  if ($pass -eq 1) { Start-Sleep -Seconds 3 }
}
Write-Output ("written " + $Out)
Write-Output ("lines=" + ([System.IO.File]::ReadAllLines($Out).Count))

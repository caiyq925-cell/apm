param(
  [Parameter(Mandatory=$true)][int]$Hwnd
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$want = [string]::Join('', [char[]](0x5F00, 0x59CB, 0x7EDF, 0x8BA1))   # kai shi tong ji

$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
if ($root -eq $null) { Write-Output "FromHandle null"; exit 1 }
Write-Output ("target: " + $want)

$cond = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Button)

$btns = $null
for ($try = 1; $try -le 8; $try++) {
  $btns = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
  if ($btns.Count -gt 0) { break }
  Start-Sleep -Seconds 2
}
foreach ($b in $btns) {
  if ($b.Current.Name -eq $want) {
    $ip = $null
    if ($b.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$ip)) {
      $ip.Invoke()
      Write-Output ("invoked (id=" + $b.Current.AutomationId + ")")
    } else { Write-Output "no InvokePattern" }
    exit 0
  }
}
Write-Output "button not found"

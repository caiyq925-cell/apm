param(
  [Parameter(Mandatory=$true)][int]$Hwnd
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$settingsTxt = [string]::Join('', [char[]](0x8BBE, 0x7F6E))                                     # she zhi
$loginBtnTxt = [string]::Join('', [char[]](0x626B, 0x7801, 0x767B, 0x5F55)) # sao ma deng lu teng xun yun

$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Hwnd)
if ($root -eq $null) { Write-Output "FromHandle returned null"; exit 1 }
Write-Output ("root: " + $root.Current.Name)

$cond = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Button)

# Chromium builds its UIA tree lazily: retry until buttons show up
$btns = $null
for ($try = 1; $try -le 8; $try++) {
  $btns = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
  Write-Output ("try " + $try + ": buttons=" + $btns.Count)
  if ($btns.Count -gt 0) { break }
  Start-Sleep -Seconds 2
}
if ($btns.Count -eq 0) { Write-Output "UIA tree stayed empty"; exit 1 }

# expand settings so the login button becomes reachable
foreach ($b in $btns) {
  $n = $b.Current.Name
  if ($n -and $n.StartsWith($settingsTxt)) {
    $p = $null
    if ($b.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$p)) {
      if ($p.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Expanded) {
        $p.Expand()
        Write-Output "expanded settings"
        Start-Sleep -Milliseconds 800
      }
    }
    break
  }
}

$btns = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
$hit = $false
foreach ($b in $btns) {
  if ($b.Current.Name -eq $loginBtnTxt) {
    try {
      $ip = $null
      if ($b.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$ip)) {
        $ip.Invoke()
        $hit = $true
        Write-Output "invoked login button"
      } else { Write-Output "no InvokePattern on login button" }
    } catch { Write-Output ("invoke failed: " + $_.Exception.Message) }
    break
  }
}
if (-not $hit) { Write-Output "login button not found" }

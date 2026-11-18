# M0 memory check on Windows.
# Usage: start Paneful (or Edge with N profiles), let timelines load, scroll a bit,
# wait ~1 min, then run:  powershell -ExecutionPolicy Bypass -File scripts\measure-windows.ps1
# Reports Paneful's process tree vs every msedge.exe, and how many WebView2
# *browser* processes Paneful owns (1 means the profile patch works: all
# accounts share one runtime; N means one runtime per account).

$all = Get-CimInstance Win32_Process
$byId = @{}; foreach ($p in $all) { $byId[[int]$p.ProcessId] = $p }

function Get-Tree([int]$rootId) {
  $ids = New-Object System.Collections.Generic.List[int]
  $ids.Add($rootId)
  $i = 0
  while ($i -lt $ids.Count) {
    $cur = $ids[$i]; $i++
    foreach ($p in $all) { if ([int]$p.ParentProcessId -eq $cur -and -not $ids.Contains([int]$p.ProcessId)) { $ids.Add([int]$p.ProcessId) } }
  }
  return $ids
}

function Sum-Mem($ids) {
  $ws = 0; $priv = 0
  foreach ($id in $ids) {
    $gp = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($gp) { $ws += $gp.WorkingSet64; $priv += $gp.PrivateMemorySize64 }
  }
  [pscustomobject]@{ Processes = $ids.Count; WorkingSetMB = [math]::Round($ws / 1MB); PrivateMB = [math]::Round($priv / 1MB) }
}

$app = $all | Where-Object { $_.Name -ieq 'paneful.exe' }
if ($app) {
  foreach ($a in $app) {
    $tree = Get-Tree ([int]$a.ProcessId)
    $browsers = @($tree | ForEach-Object { $byId[$_] } | Where-Object { $_.Name -ieq 'msedgewebview2.exe' -and $_.CommandLine -notmatch '--type=' })
    Write-Host "== Paneful (pid $($a.ProcessId))"
    Sum-Mem $tree | Format-List
    Write-Host "WebView2 browser processes: $($browsers.Count)  (1 = shared runtime OK)"
  }
} else { Write-Host "Paneful not running" }

$edge = @($all | Where-Object { $_.Name -ieq 'msedge.exe' } | ForEach-Object { [int]$_.ProcessId })
if ($edge.Count) {
  Write-Host "== Edge (all msedge.exe)"
  Sum-Mem $edge | Format-List
} else { Write-Host "Edge not running" }

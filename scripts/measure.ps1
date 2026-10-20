# Launch the overlay, sample memory / CPU / context switches while awake and asleep,
# compare against the budgets, and write target/perf-report.json.
#
# Usage:
#   .\scripts\measure.ps1                 # measure and print
#   .\scripts\measure.ps1 -Enforce        # also exit 1 if any budget is exceeded
#
# The spike stays awake for 15 s after launch, then sleeps (timer killed).
param(
    [string]$Exe = "$PSScriptRoot\..\target\release\overlay-spike.exe",
    [int]$AwakeSampleSecs = 8,
    [int]$SleepSampleSecs = 20,
    [switch]$Enforce
)

# Budgets (see docs/SPIKE-RESULTS.md and the plan). Tighten as we learn.
$Budget = @{
    PrivateMBMax        = 25     # private memory, awake or asleep
    AsleepCpuPercentMax = 0.05   # % of all cores, averaged over the sample
    AsleepCtxSwitchMax  = 2.0    # context switches per second across all threads (wakeup proxy)
    AwakeCpuPercentMax  = 1.0
}

$Exe = (Resolve-Path $Exe).Path
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 2

function Get-CtxSwitchesPerSec($procName, $seconds) {
    # Sum of per-thread "Context Switches/sec" for this process. Best-effort: returns $null if unavailable.
    # One call per second: a long multi-sample call fails if a thread instance appears or vanishes mid-run.
    $totals = @()
    for ($i = 0; $i -lt $seconds; $i++) {
        try {
            $c = Get-Counter -Counter "\Thread($procName*)\Context Switches/sec" -ErrorAction Stop
            $totals += ($c.CounterSamples | Measure-Object CookedValue -Sum).Sum
        } catch { Start-Sleep -Seconds 1 }
    }
    if ($totals.Count -eq 0) { return $null }
    return [math]::Round(($totals | Measure-Object -Average).Average, 2)
}

function Sample($label, $seconds) {
    $p.Refresh()
    $cpu0 = $p.TotalProcessorTime.TotalMilliseconds
    $t0 = Get-Date
    $ctx = Get-CtxSwitchesPerSec $p.ProcessName $seconds
    $p.Refresh()
    $elapsed = ((Get-Date) - $t0).TotalSeconds
    $cpuMs = $p.TotalProcessorTime.TotalMilliseconds - $cpu0
    [pscustomobject]@{
        State         = $label
        PrivateMB     = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
        WorkingSetMB  = [math]::Round($p.WorkingSet64 / 1MB, 1)
        CpuPercent    = [math]::Round(100 * $cpuMs / ($elapsed * 1000 * [Environment]::ProcessorCount), 3)
        CtxSwitchPerS = $ctx
        Threads       = $p.Threads.Count
    }
}

try {
    $awake = Sample "awake" $AwakeSampleSecs
    # Wait until the spike's awake window has certainly ended.
    $remaining = 17 - ((Get-Date) - $p.StartTime).TotalSeconds
    if ($remaining -gt 0) { Start-Sleep -Seconds ([math]::Ceiling($remaining)) }
    $asleep = Sample "asleep" $SleepSampleSecs
} finally {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}

$rows = @($awake, $asleep)
$rows | Format-Table -AutoSize

$failures = @()
foreach ($r in $rows) {
    if ($r.PrivateMB -gt $Budget.PrivateMBMax) { $failures += "$($r.State): private $($r.PrivateMB) MB > $($Budget.PrivateMBMax)" }
}
if ($awake.CpuPercent -gt $Budget.AwakeCpuPercentMax) { $failures += "awake: CPU $($awake.CpuPercent)% > $($Budget.AwakeCpuPercentMax)" }
if ($asleep.CpuPercent -gt $Budget.AsleepCpuPercentMax) { $failures += "asleep: CPU $($asleep.CpuPercent)% > $($Budget.AsleepCpuPercentMax)" }
if ($null -eq $asleep.CtxSwitchPerS) {
    Write-Warning "Context-switch counter unavailable; wakeups not checked."
} elseif ($asleep.CtxSwitchPerS -gt $Budget.AsleepCtxSwitchMax) {
    $failures += "asleep: $($asleep.CtxSwitchPerS) ctx switches/s > $($Budget.AsleepCtxSwitchMax)"
}

$outDir = Join-Path $PSScriptRoot "..\target"
New-Item -ItemType Directory -Force $outDir | Out-Null
[pscustomobject]@{ budget = $Budget; samples = $rows; failures = $failures } |
    ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $outDir "perf-report.json")

if ($failures.Count -gt 0) {
    Write-Host "BUDGET EXCEEDED:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    if ($Enforce) { exit 1 }
} else {
    Write-Host "All budgets met." -ForegroundColor Green
}

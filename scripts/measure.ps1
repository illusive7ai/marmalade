# Launch the spike, then sample memory and CPU while awake and while asleep.
# Usage: .\scripts\measure.ps1 [-Exe path] [-AwakeSecs 15] [-SleepSampleSecs 20]
param(
    [string]$Exe = "$PSScriptRoot\..\target\release\overlay-spike.exe",
    [int]$AwakeSecs = 15,
    [int]$SleepSampleSecs = 20
)

$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 2

function Sample($label, $seconds) {
    $p.Refresh()
    $cpu0 = $p.TotalProcessorTime.TotalMilliseconds
    $t0 = Get-Date
    Start-Sleep -Seconds $seconds
    $p.Refresh()
    $cpuMs = $p.TotalProcessorTime.TotalMilliseconds - $cpu0
    $elapsed = ((Get-Date) - $t0).TotalSeconds
    [pscustomobject]@{
        State        = $label
        PrivateMB    = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
        WorkingSetMB = [math]::Round($p.WorkingSet64 / 1MB, 1)
        CpuPercent   = [math]::Round(100 * $cpuMs / ($elapsed * 1000 * [Environment]::ProcessorCount), 3)
        Threads      = $p.Threads.Count
    }
}

$awake = Sample "awake (30fps)" 8
Start-Sleep -Seconds ([math]::Max(0, $AwakeSecs - 8))
$asleep = Sample "asleep" $SleepSampleSecs
@($awake, $asleep) | Format-Table -AutoSize

Stop-Process -Id $p.Id -Force

param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$TempRoot
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
if ($env:HORIZON_REQUIRE_REDIS -ne '1') { throw 'A dedicated Redis fixture is required.' }
$env:APP_ENV = 'testing'
$env:LOG_CHANNEL = 'null'
$env:HORIZON_PREFIX = 'windows-process:'
$env:HORIZON_PROBE_MASTER_BASENAME = 'North:' + [char]0x6771 + [char]0x4eac + ' 1'
$env:HORIZON_PROBE_WORKER_TIMEOUT = '22'
$env:HORIZON_PROBE_BLOCK_FOR = '20'
Remove-Item Env:HORIZON_PROBE_EMPTY_DEFAULTS -ErrorAction SilentlyContinue

function Artisan {
    $output = & $Binary artisan @args
    if ($LASTEXITCODE -ne 0) { throw "Artisan $args failed with code $LASTEXITCODE. $output" }
    return $output
}

function Status {
    $output = & $Binary artisan horizon:status --json
    if ($LASTEXITCODE -notin @(0, 1, 2)) { throw "Status failed with code $LASTEXITCODE." }
    return (ConvertFrom-Json -InputObject ($output -join "`n"))
}

function Wait-For($Description, $Condition, $Seconds = 20) {
    $until = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        $value = & $Condition
        if ($value) { return $value }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $until)
    throw "Timed out waiting for $Description. $(Status | ConvertTo-Json -Depth 6 -Compress)"
}

function Dispatch([int]$Duration, [string]$OutputMarker) {
    $payload = @{sleep_ms = $Duration; record_start = $true}
    if ($OutputMarker) {
        $payload.probe_output = $OutputMarker
        $payload.probe_format = $OutputMarker
    }
    $env:HORIZON_PROBE_DISPATCH = $payload | ConvertTo-Json -Compress
    $env:HORIZON_PROBE_DISPATCH_UUID = '1'
    try {
        $output = & $Binary
        if ($LASTEXITCODE -ne 0) { throw "Worker dispatch failed with code $LASTEXITCODE." }
        return $output.Trim()
    } finally {
        Remove-Item Env:HORIZON_PROBE_DISPATCH, Env:HORIZON_PROBE_DISPATCH_UUID
    }
}

function Recorded($File, $Job) {
    $path = Join-Path $env:APP_BASE_PATH $File
    return (Test-Path $path) -and (@(Get-Content $path) -contains $Job)
}

function Supervisor-Command($Name, $Command, $Options, [switch]$MasterQueue) {
    $env:HORIZON_PROBE_COMMAND_QUEUE = @{
        action = 'push'; name = $Name; master = $MasterQueue.IsPresent
        bound = $env:HORIZON_PROBE_CUSTOM_COMMAND_QUEUE -in @('1', 'file')
        command = 'Laravel\Horizon\SupervisorCommands\' + $Command
        options = $Options
    } | ConvertTo-Json -Depth 4 -Compress
    try {
        & $Binary
        if ($LASTEXITCODE -ne 0) { throw "Could not enqueue Supervisor command $Command." }
    } finally {
        Remove-Item Env:HORIZON_PROBE_COMMAND_QUEUE
    }
}

$env:HORIZON_PROBE_WRAPPER_OUTPUT = '1'
try {
    & $Binary
    if ($LASTEXITCODE -ne 0) { throw 'The native process wrapper probe failed.' }
} finally {
    Remove-Item Env:HORIZON_PROBE_WRAPPER_OUTPUT
}
$env:HORIZON_PROBE_WRAPPER_CONTROL = '1'
try {
    & $Binary
    if ($LASTEXITCODE -ne 0) { throw 'The native process wrapper control probe failed.' }
} finally {
    Remove-Item Env:HORIZON_PROBE_WRAPPER_CONTROL
}

foreach ($mode in @('idle', 'wait', 'fast', 'restart-fast', 'parent-exit', 'handlers', 'queue', 'file-queue')) {
    $customQueue = $mode -in @('queue', 'file-queue')
    $fastRestart = $mode -eq 'restart-fast'
    $emptyPlan = $customQueue -or $mode -in @('idle', 'handlers', 'restart-fast')
    $fixture = Join-Path $TempRoot "horizon-$mode-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory $fixture | Out-Null
    New-Item -ItemType File (Join-Path $fixture 'database.sqlite') | Out-Null
    $env:APP_BASE_PATH = $fixture
    $env:HORIZON_PROBE_FAST_TERMINATION = if ($mode -in @('wait', 'fast', 'restart-fast')) { '1' } else { '0' }
    $env:HORIZON_PROBE_EMPTY_DEFAULTS = if ($emptyPlan) { '1' } else { '0' }
    $env:HORIZON_PROBE_COMMAND_HANDLERS = if ($mode -eq 'handlers') { '1' } else { $null }
    $env:HORIZON_PROBE_CUSTOM_COMMAND_QUEUE = switch ($mode) {
        'queue' { '1' }
        'file-queue' { 'file' }
        default { $null }
    }
    $env:HORIZON_PROBE_PRELOAD_MASTER_COMMAND = if ($mode -eq 'idle') { '1' } else { $null }
    $env:HORIZON_PROBE_PREPARE_DATABASE = '1'
    try {
        & $Binary
        if ($LASTEXITCODE -ne 0) { throw 'Could not create the test failed_jobs table.' }
    } finally {
        Remove-Item Env:HORIZON_PROBE_PREPARE_DATABASE
    }
    $master = $null
    $worker = $null
    $supervisor = $null
    try {
        $master = Start-Process -FilePath $Binary -ArgumentList 'artisan', 'horizon' -NoNewWindow -PassThru `
            -RedirectStandardOutput (Join-Path $fixture 'master.stdout') `
            -RedirectStandardError (Join-Path $fixture 'master.stderr')
        $null = $master.Handle
        if ($emptyPlan) {
            $empty = Wait-For 'empty Master' {
                $row = Status
                if ($row.pid -eq $master.Id) { $row }
            }
            if ($empty.supervisors.Count -ne 0) { throw 'A command survived the startup flush.' }
            $queuedDeployment = @{
                action = 'push'; name = $empty.name; master = $true
                bound = $customQueue
                options = @{
                    name = $empty.name + ':supervisor-1'; queue = @('default')
                    maxProcesses = 1; balanceCooldown = 120
                }
            }
            if ($mode -eq 'handlers') { $queuedDeployment.command = 'Probe\Deploy' }
            $env:HORIZON_PROBE_COMMAND_QUEUE = $queuedDeployment | ConvertTo-Json -Depth 4 -Compress
            try {
                & $Binary
                if ($LASTEXITCODE -ne 0) { throw 'Public Master command enqueue failed.' }
            } finally {
                Remove-Item Env:HORIZON_PROBE_COMMAND_QUEUE
            }
        }
        $active = Wait-For 'native Worker' {
            $row = Status
            if ($row.pid -eq $master.Id -and @($row.workers).Count -eq 1) { $row }
        }
        if ($active.basename -cne $env:HORIZON_PROBE_MASTER_BASENAME) { throw 'Master name changed.' }
        $name = $active.supervisors[0]
        $worker = Get-Process -Id $active.workers[0].pid
        $null = $worker.Handle
        $child = Get-CimInstance Win32_Process -Filter "ProcessId = $($worker.Id)"
        $supervisor = Get-Process -Id $child.ParentProcessId
        $null = $supervisor.Handle
        $child = Get-CimInstance Win32_Process -Filter "ProcessId = $($supervisor.Id)"
        if ($child.ParentProcessId -ne $master.Id -or $name -cne ($active.name + ':supervisor-1')) {
            throw 'Unexpected native process ancestry.'
        }
        $null = Artisan horizon:supervisor-status $name

        if ($customQueue -or $mode -in @('idle', 'restart-fast')) {
            $dynamic = Dispatch 20 'windows-native'
            Wait-For 'job on the dynamically added Worker' { Recorded 'completed.log' $dynamic } | Out-Null
            Wait-For 'both Worker streams on Master stdout' {
                $contents = Get-Content (Join-Path $fixture 'master.stdout') -Raw -Encoding UTF8
                $contents -match 'worker-out-.*-windows-native' -and
                    $contents.Contains('worker-err-windows-native') -and
                    $contents.Contains('styled-windows-native <unknown>windows-native</unknown>')
            } | Out-Null
            'PASS: Windows native Horizon streamed both Worker output streams through the Master'

            $duration = if ($fastRestart) { 12000 } else { 6000 }
            $draining = Dispatch $duration
            Wait-For 'job running before Master restart' { Recorded 'started.log' $draining } | Out-Null
            $null = Artisan horizon:pause
            Wait-For 'paused Master' { (Status).status -eq 'paused' } | Out-Null
            $queued = Dispatch 20
            Supervisor-Command $active.name Restart @() -MasterQueue
            $restarted = Wait-For 'Master with replacement Worker' {
                $row = Status
                if ($row.pid -eq $master.Id -and $row.status -eq 'running' -and
                    @($row.workers).Count -eq 1 -and $row.workers[0].pid -ne $worker.Id) { $row }
            }
            if (!$supervisor.WaitForExit(15000) -or $supervisor.ExitCode -ne 1) { throw 'Master restart did not exit the old Supervisor with status 1.' }
            Wait-For 'job on the replacement Worker' { Recorded 'completed.log' $queued } | Out-Null
            if (!$worker.WaitForExit(15000)) { throw 'The previous Worker remained alive.' }
            $worker = Get-Process -Id $restarted.workers[0].pid
            $null = $worker.Handle
            $parent = Get-CimInstance Win32_Process -Filter "ProcessId = $($worker.Id)"
            if ($parent.ParentProcessId -eq $supervisor.Id) { throw 'Master restart kept the previous Supervisor.' }
            $supervisor = Get-Process -Id $parent.ParentProcessId
            $null = $supervisor.Handle
            $finished = @(Get-Content (Join-Path $fixture 'completed.log'))
            if (@($finished | Where-Object { $_ -eq $draining }).Count -ne 1 -or
                @($finished | Where-Object { $_ -eq $queued }).Count -ne 1) {
                throw 'Master restart did not finish both jobs exactly once.'
            }
            $drainedFirst = [array]::IndexOf($finished, $draining) -lt [array]::IndexOf($finished, $queued)
            if ($drainedFirst -eq $fastRestart) {
                throw "Unexpected job completion order during Master restart ($mode)."
            }
            'PASS: Windows native Master restart resumed and replaced the managed process tree'

            Supervisor-Command $name Pause @()
            Wait-For 'structured pause' { ((Artisan horizon:supervisor-status $name) -join "`n") -match 'is paused' } | Out-Null
            Supervisor-Command $name ContinueWorking @()
            Wait-For 'structured continue' { ((Artisan horizon:supervisor-status $name) -join "`n") -match 'is running' } | Out-Null
            Supervisor-Command $name Restart @()
            Wait-For 'structured Worker restart' { @((Status).workers | Where-Object { !$_.terminating -and $_.pid -ne $worker.Id }).Count -eq 1 } | Out-Null
            Supervisor-Command $name Scale @{scale = 2}
            Wait-For 'structured scale' { @((Status).workers | Where-Object { !$_.terminating }).Count -eq 2 } | Out-Null
            Supervisor-Command $name Balance @{default = 1; unknown = 5}
            $balancedWorker = Wait-For 'structured balance' {
                $running = @((Status).workers | Where-Object { !$_.terminating })
                if ($running.Count -eq 1) { $running[0] }
            }
            $worker = Get-Process -Id $balancedWorker.pid
            $null = $worker.Handle
            Supervisor-Command $name Terminate @()
            if (!$supervisor.WaitForExit(15000) -or $supervisor.ExitCode -ne 0) { throw 'Structured Terminate did not exit with status 0.' }
            'PASS: Windows native structured Supervisor commands'
        }

        if ($mode -eq 'wait') {
            $null = Artisan horizon:pause
            Wait-For 'paused Supervisor' { ((Artisan horizon:supervisor-status $name) -join "`n") -match 'is paused' } | Out-Null
            $paused = Dispatch 20
            Start-Sleep -Seconds 2
            if (Recorded 'started.log' $paused) { throw 'A paused Worker reserved the job.' }
            $null = Artisan horizon:continue
            Wait-For 'job resumed' { Recorded 'completed.log' $paused } | Out-Null
        }

        if ($mode -eq 'handlers') {
            Supervisor-Command $name Pause @{mode = 'override'; nested = @($false, 'observed')}
            Wait-For 'named command handlers' {
                $journal = Join-Path $fixture 'command-handlers.jsonl'
                if (!(Test-Path $journal)) { return }
                $rows = @(Get-Content -Encoding UTF8 $journal | ForEach-Object { ConvertFrom-Json $_ })
                $rows.Count -eq 2 -and $rows[0].command -ceq 'Probe\Deploy' -and
                    $rows[1].name -ceq $name -and $rows[1].options.nested[1] -ceq 'observed'
            } | Out-Null
            if (((Artisan horizon:supervisor-status $name) -join "`n") -notmatch 'is running') {
                throw 'The built-in Pause ran instead of the named application handler.'
            }
            $continued = Dispatch 20
            Wait-For 'job after the named Pause override' { Recorded 'completed.log' $continued } | Out-Null
            'PASS: Windows native named Master handler and Supervisor Pause override'
        }

        if ($mode -in @('handlers', 'file-queue')) {
            $env:HORIZON_PROBE_SUPERVISOR_MONITOR = $active.name
            try {
                & $Binary
                if ($LASTEXITCODE -ne 0) { throw 'The supervisor monitor failed.' }
            } finally {
                Remove-Item Env:HORIZON_PROBE_SUPERVISOR_MONITOR
            }
            Wait-For 'supervisor reprovisioned by its independent monitor' {
                $workers = (Status).workers
                $defaults = @($workers | Where-Object {
                    $_.supervisor -ceq ($active.name + ':monitor-reprovision') -and $_.queue -eq 'wrapper-monitor'
                })
                $override = @($workers | Where-Object {
                    $_.supervisor -ceq ($active.name + ':monitor-terminal-override') -and $_.queue -eq 'wrapper-monitor-custom'
                })
                $defaults.Count -eq 1 -and $override.Count -eq 1
            } | Out-Null
            'PASS: Windows native Supervisor monitor reprovisioned original options under both exit policies'
        }

        if (!$emptyPlan) {
            $job = Dispatch 12000
            Wait-For 'job in progress' { Recorded 'started.log' $job } | Out-Null
        }
        if ($mode -eq 'parent-exit') { $pending = Dispatch 20 }
        switch ($mode) {
            'idle' { $null = Artisan horizon:terminate }
            'handlers' { $null = Artisan horizon:terminate }
            'queue' { $null = Artisan horizon:terminate }
            'file-queue' { $null = Artisan horizon:terminate }
            'wait' { $null = Artisan horizon:terminate --wait }
            'fast' { $null = Artisan horizon:terminate }
            'restart-fast' { $null = Artisan horizon:terminate }
            'parent-exit' { $master.Kill() }
        }
        if (!$master.WaitForExit(25000)) { throw 'Master did not exit.' }
        if ($mode -ne 'parent-exit' -and $master.ExitCode -ne 0) { throw 'Master exited with an error.' }
        if (!$emptyPlan) {
            $finished = Recorded 'completed.log' $job
            if ($mode -eq 'wait' -and !$finished) { throw '--wait left a running job.' }
            if ($mode -ne 'wait' -and $finished) { throw 'Master exited after its running job.' }
            Wait-For 'completion of the same job' { Recorded 'completed.log' $job } | Out-Null
        }
        if (!$supervisor.WaitForExit(15000) -or !$worker.WaitForExit(15000)) { throw 'Managed processes remained alive.' }
        if ($mode -eq 'parent-exit') {
            if (Recorded 'started.log' $pending) { throw 'An orphaned Worker reserved another job.' }
            $null = Artisan queue:work redis --once --force
            if (!(Recorded 'completed.log' $pending)) { throw 'The pending job could not be recovered.' }
        }
        if ($mode -ne 'parent-exit' -and (Status)) { throw 'Stopped Master remained in Redis.' }
        if (((Artisan horizon:supervisors) -join "`n") -notmatch 'No supervisors are running') { throw 'Stopped Supervisor remained in Redis.' }
        "PASS: Windows native process tree $mode"
    } finally {
        foreach ($process in @($master, $supervisor, $worker)) {
            if ($process -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        }
        Get-Content -ErrorAction SilentlyContinue (Join-Path $fixture 'master.stderr')
    }
}

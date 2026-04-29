param(
    [string]$Task = "Create a file named codex-real-rerun-smoke.txt containing exactly: Keel scripted real Codex rerun smoke test",
    [int]$AgentTimeoutSecs = 900
)

$ErrorActionPreference = "Stop"

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [string[]]$Arguments = @(),
        [string]$WorkingDirectory = (Get-Location).Path
    )

    $stdoutPath = [System.IO.Path]::GetTempFileName()
    $stderrPath = [System.IO.Path]::GetTempFileName()
    $argumentLine = Join-ProcessArguments $Arguments
    try {
        $process = Start-Process `
            -FilePath $FilePath `
            -ArgumentList $argumentLine `
            -WorkingDirectory $WorkingDirectory `
            -NoNewWindow `
            -Wait `
            -PassThru `
            -RedirectStandardOutput $stdoutPath `
            -RedirectStandardError $stderrPath

        $stdout = Get-Content -LiteralPath $stdoutPath -Raw
        $stderr = Get-Content -LiteralPath $stderrPath -Raw
        if ($process.ExitCode -ne 0) {
            $joined = "$stdout`n$stderr".Trim()
            throw "Command failed ($($process.ExitCode)): $FilePath $argumentLine`n$joined"
        }

        return @($stdout -split "`r?`n" | Where-Object { $_ -ne "" }) + @($stderr -split "`r?`n" | Where-Object { $_ -ne "" })
    }
    finally {
        Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
    }
}

function Join-ProcessArguments {
    param([string[]]$Arguments)

    return ($Arguments | ForEach-Object { ConvertTo-ProcessArgument $_ }) -join " "
}

function ConvertTo-ProcessArgument {
    param([string]$Argument)

    if ($Argument -notmatch '[\s"]') {
        return $Argument
    }

    return '"' + ($Argument -replace '"', '\"') + '"'
}

function Get-RunId {
    param([string[]]$Output)

    $line = $Output | Where-Object { $_ -match "^(Run created|Rerun created): " } | Select-Object -First 1
    if (-not $line) {
        throw "Could not parse run id from output:`n$(($Output | Out-String).TrimEnd())"
    }

    return ($line -replace "^(Run created|Rerun created): ", "").Trim()
}

function Assert-PathExists {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Expected path to exist: $Path"
    }
}

function Assert-PathMissing {
    param([string]$Path)

    if (Test-Path -LiteralPath $Path) {
        throw "Expected path to be removed: $Path"
    }
}

function Assert-RunArtifacts {
    param(
        [string]$Repo,
        [string]$RunId,
        [string]$ExpectedDiffPath
    )

    $runDir = Join-Path $Repo ".keel\runs\$RunId"
    Assert-PathExists (Join-Path $runDir "metadata.json")
    Assert-PathExists (Join-Path $runDir "log.txt")
    Assert-PathExists (Join-Path $runDir "diff.patch")
    Assert-PathExists (Join-Path $runDir "checks.json")
    Assert-PathExists (Join-Path $runDir "report.md")

    $metadata = Get-Content -LiteralPath (Join-Path $runDir "metadata.json") -Raw | ConvertFrom-Json
    if ($metadata.agent -ne "codex") {
        throw "Expected codex agent for $RunId, got $($metadata.agent)"
    }
    if ($metadata.status -ne "ready") {
        throw "Expected ready status for $RunId, got $($metadata.status)"
    }

    $diff = Get-Content -LiteralPath (Join-Path $runDir "diff.patch") -Raw
    if (-not $diff.Contains($ExpectedDiffPath)) {
        throw "Expected diff for $RunId to mention $ExpectedDiffPath"
    }

    return $metadata
}

$codex = Get-Command codex -ErrorAction SilentlyContinue
if (-not $codex) {
    throw "codex CLI not found; install Codex CLI or ensure codex is on PATH"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$isWindows = [System.IO.Path]::DirectorySeparatorChar -eq "\"
$keelBinary = if ($isWindows) { "keel.exe" } else { "keel" }
$keelExe = Join-Path (Join-Path (Join-Path $repoRoot "target") "debug") $keelBinary

Invoke-Checked -FilePath "cargo" -Arguments @("build", "-p", "keel-cli") -WorkingDirectory $repoRoot | Out-Null

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "keel-real-codex-rerun-smoke-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    Invoke-Checked -FilePath "git" -Arguments @("init") -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath "git" -Arguments @("config", "user.email", "keel-smoke@example.invalid") -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath "git" -Arguments @("config", "user.name", "Keel Smoke") -WorkingDirectory $tempRoot | Out-Null
    Set-Content -LiteralPath (Join-Path $tempRoot "README.md") -Value "# Keel smoke repo`n" -Encoding ASCII
    Invoke-Checked -FilePath "git" -Arguments @("add", "README.md") -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath "git" -Arguments @("commit", "-m", "initial") -WorkingDirectory $tempRoot | Out-Null

    Invoke-Checked -FilePath $keelExe -Arguments @("init") -WorkingDirectory $tempRoot | Out-Null
    Set-Content -LiteralPath (Join-Path $tempRoot ".keel\config.toml") -Value @"
agent_timeout_secs = $AgentTimeoutSecs

[[checks]]
name = "git status"
command = ["git", "status", "--short"]
"@ -Encoding ASCII

    $runOutput = Invoke-Checked -FilePath $keelExe -Arguments @("run", $Task, "--agent", "codex") -WorkingDirectory $tempRoot
    $sourceRunId = Get-RunId $runOutput
    $rerunOutput = Invoke-Checked -FilePath $keelExe -Arguments @("rerun", $sourceRunId) -WorkingDirectory $tempRoot
    $childRunId = Get-RunId $rerunOutput

    $sourceMetadata = Assert-RunArtifacts $tempRoot $sourceRunId "codex-real-rerun-smoke.txt"
    $childMetadata = Assert-RunArtifacts $tempRoot $childRunId "codex-real-rerun-smoke.txt"
    if ($childMetadata.parent_run_id -ne $sourceRunId) {
        throw "Expected child parent_run_id $sourceRunId, got $($childMetadata.parent_run_id)"
    }
    if ($sourceMetadata.worktree_path -eq $childMetadata.worktree_path) {
        throw "Rerun reused the source worktree path"
    }

    Invoke-Checked -FilePath $keelExe -Arguments @("status") -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath $keelExe -Arguments @("report", $sourceRunId) -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath $keelExe -Arguments @("report", $childRunId) -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath $keelExe -Arguments @("discard", $sourceRunId) -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath $keelExe -Arguments @("discard", $childRunId) -WorkingDirectory $tempRoot | Out-Null

    Assert-PathMissing (Join-Path $tempRoot ".keel\worktrees\$sourceRunId")
    Assert-PathMissing (Join-Path $tempRoot ".keel\worktrees\$childRunId")
    Assert-PathExists (Join-Path $tempRoot ".keel\runs\$sourceRunId\report.md")
    Assert-PathExists (Join-Path $tempRoot ".keel\runs\$childRunId\report.md")

    Write-Output "REAL_CODEX_RERUN_SMOKE_OK repo=$tempRoot source=$sourceRunId child=$childRunId"
}
catch {
    Write-Error $_
    exit 1
}

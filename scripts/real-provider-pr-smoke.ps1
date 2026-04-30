param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("github", "gitlab")]
    [string]$Provider,

    [Parameter(Mandatory = $true)]
    [string]$Remote,

    [string]$Target = "main",

    [switch]$CloseRequest
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

    $line = $Output | Where-Object { $_ -match "^Run created: " } | Select-Object -First 1
    if (-not $line) {
        throw "Could not parse run id from output:`n$(($Output | Out-String).TrimEnd())"
    }

    return ($line -replace "^Run created: ", "").Trim()
}

function Assert-ProviderReady {
    param([string]$Provider)

    $cli = if ($Provider -eq "github") { "gh" } else { "glab" }
    if (-not (Get-Command $cli -ErrorAction SilentlyContinue)) {
        throw "$cli CLI not found; install it and authenticate before running this smoke test"
    }

    Invoke-Checked -FilePath $cli -Arguments @("auth", "status") | Out-Null
    return $cli
}

function Close-ProviderRequest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Provider,
        [Parameter(Mandatory = $true)]
        [string]$Cli,
        [Parameter(Mandatory = $true)]
        [string]$Url
    )

    if ($Provider -eq "github") {
        Invoke-Checked -FilePath $Cli -Arguments @("pr", "close", $Url, "--delete-branch=false") | Out-Null
        return
    }

    Invoke-Checked -FilePath $Cli -Arguments @("mr", "close", $Url) | Out-Null
}

$cli = Assert-ProviderReady $Provider
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$isWindows = [System.IO.Path]::DirectorySeparatorChar -eq "\"
$keelBinary = if ($isWindows) { "keel.exe" } else { "keel" }
$keelExe = Join-Path (Join-Path (Join-Path $repoRoot "target") "debug") $keelBinary

Invoke-Checked -FilePath "cargo" -Arguments @("build", "-p", "keel-cli") -WorkingDirectory $repoRoot | Out-Null

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "keel-real-$Provider-pr-smoke-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    Invoke-Checked -FilePath "git" -Arguments @("clone", $Remote, ".") -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath "git" -Arguments @("config", "user.email", "keel-real-pr@example.local") -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath "git" -Arguments @("config", "user.name", "Keel Real PR Smoke") -WorkingDirectory $tempRoot | Out-Null

    Invoke-Checked -FilePath $keelExe -Arguments @("init") -WorkingDirectory $tempRoot | Out-Null
    $runOutput = Invoke-Checked -FilePath $keelExe -Arguments @("run", "real $Provider PR smoke", "--agent", "noop") -WorkingDirectory $tempRoot
    $runId = Get-RunId $runOutput

    Invoke-Checked -FilePath $keelExe -Arguments @("commit", $runId) -WorkingDirectory $tempRoot | Out-Null
    Invoke-Checked -FilePath $keelExe -Arguments @("push", $runId) -WorkingDirectory $tempRoot | Out-Null
    $prOutput = Invoke-Checked -FilePath $keelExe -Arguments @("pr", $runId, "--provider", $Provider, "--target", $Target) -WorkingDirectory $tempRoot

    $prPath = Join-Path $tempRoot ".keel\runs\$runId\pr.json"
    if (-not (Test-Path -LiteralPath $prPath)) {
        throw "Expected pr.json to be written at $prPath"
    }

    $pr = Get-Content -LiteralPath $prPath -Raw | ConvertFrom-Json
    if (-not $pr.url -or -not $pr.url.StartsWith("http")) {
        throw "Expected provider PR/MR URL in pr.json"
    }

    Write-Output "REAL_PROVIDER_PR_SMOKE_OK provider=$Provider cli=$cli repo=$tempRoot run=$runId url=$($pr.url)"
    Write-Output ($prOutput | Out-String).TrimEnd()

    if ($CloseRequest) {
        Close-ProviderRequest -Provider $Provider -Cli $cli -Url $pr.url
        Write-Output "REAL_PROVIDER_PR_SMOKE_CLOSED provider=$Provider url=$($pr.url)"
    }
}
catch {
    Write-Error $_
    exit 1
}

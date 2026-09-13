# Fetch and prepare a complete Wenyi engine checkout for Wenyi Desktop.
#
# Clones the upstream repository at the exact revision this repository's P0 patch was
# generated against, then applies the patch. The result is a full, buildable engine --
# no manual steps and no dependency on anyone's working tree.
#
# Usage:
#   .\bootstrap_engine.ps1                          # -> ..\wenyi-engine
#   .\bootstrap_engine.ps1 -Dest D:\src\wenyi
#   .\bootstrap_engine.ps1 -Proxy http://127.0.0.1:10808
param(
    [string]$Dest,
    [string]$Proxy = $env:WENYI_GIT_PROXY,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

# Windows PowerShell 5.1 turns native stderr into a terminating error under
# ErrorActionPreference=Stop. Merging the streams is not enough: the merged records are
# still NativeCommandError, so relax the preference locally and check the exit code.
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments, [string]$What)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        & $Exe @Arguments 2>&1 | ForEach-Object { Write-Host "  $_" }
        $code = $LASTEXITCODE
    }
    finally { $ErrorActionPreference = $previous }
    if ($code -ne 0) { throw "$What failed (exit code $code)" }
}

function Get-GitProxyArgs {
    if ([string]::IsNullOrWhiteSpace($Proxy)) { return @() }
    Write-Host "  (using proxy $Proxy)"
    return @("-c", "http.proxy=$Proxy", "-c", "https.proxy=$Proxy")
}

$pin = Get-Content (Join-Path $PSScriptRoot "upstream.json") -Raw | ConvertFrom-Json
if (-not $Dest) { $Dest = Join-Path $PSScriptRoot "..\..\wenyi-engine" }
$Dest = [System.IO.Path]::GetFullPath($Dest)

Write-Host "Upstream : $($pin.upstream)"
Write-Host "Commit   : $($pin.baseCommitShort)  ($($pin.baseCommitSubject))"
Write-Host "Dest     : $Dest"
Write-Host ""

if (Test-Path (Join-Path $Dest ".git")) {
    if (-not $Force) {
        Write-Host "Engine checkout already exists; applying/refreshing the patch only."
        & (Join-Path $PSScriptRoot "apply.ps1") -RepoRoot $Dest
        exit $LASTEXITCODE
    }
    Write-Host "Removing existing checkout (-Force) ..."
    Remove-Item -Recurse -Force $Dest
}

# 1. Clone and pin to the exact revision.
$parent = Split-Path -Parent $Dest
New-Item -ItemType Directory -Force -Path $parent | Out-Null
Write-Host "[1/3] Cloning upstream ..."
# `-c key=value` is a git-level option and must precede the subcommand.
$gitOpts = Get-GitProxyArgs
Invoke-Native git ($gitOpts + @("clone", $pin.upstream, $Dest)) "git clone"

Push-Location $Dest
try {
    Write-Host "[2/3] Checking out pinned revision $($pin.baseCommitShort) ..."
    Invoke-Native git ($gitOpts + @("checkout", "--quiet", $pin.baseCommit)) "git checkout"
}
finally { Pop-Location }

# 2. Apply the P0 patch.
Write-Host "[3/3] Applying the P0 JSON-events patch ..."
& (Join-Path $PSScriptRoot "apply.ps1") -RepoRoot $Dest

Write-Host ""
Write-Host "Engine ready at $Dest"
Write-Host "Build the sidecar with:"
Write-Host "  .\sidecar\build_sidecar.ps1 -RepoRoot `"$Dest`""

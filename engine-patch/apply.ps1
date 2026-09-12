# Apply the P0 JSON-events patch to a Wenyi engine checkout.
#
# Usage:
#   .\apply.ps1
#   .\apply.ps1 -RepoRoot D:\path\to\wenyi
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\wenyi")).Path
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

if (-not (Test-Path (Join-Path $RepoRoot "trans_novel\cli.py"))) {
    throw "Not a Wenyi checkout: $RepoRoot"
}

# Warn (do not fail) when the checkout is not the revision the patch targets: the patch
# may still apply, but a silent mismatch is how a broken engine gets shipped.
$pin = Get-Content (Join-Path $PSScriptRoot "upstream.json") -Raw | ConvertFrom-Json
Push-Location $RepoRoot
try {
    $head = (& git rev-parse HEAD 2>$null)
    if ($LASTEXITCODE -eq 0 -and $head -and ($head.Trim() -ne $pin.baseCommit)) {
        Write-Warning "Engine HEAD is $($head.Trim().Substring(0,7)); the patch targets $($pin.baseCommitShort)."
        Write-Warning "If the patch fails, run bootstrap_engine.ps1 for a matching checkout."
    }
}
finally { Pop-Location }

Push-Location $RepoRoot
try {
    # 1. Patch the existing CLI (idempotent: skip when already applied).
    $cli = Get-Content "trans_novel\cli.py" -Raw
    if ($cli -match "--json-events") {
        Write-Host "cli.py already contains --json-events; skipping patch."
    } else {
        Write-Host "Applying cli-json-events.patch ..."
        Invoke-Native git @("apply", (Join-Path $PSScriptRoot "cli-json-events.patch")) "git apply"
    }

    # 2. Drop in the new module and tests.
    Copy-Item -Force (Join-Path $PSScriptRoot "json_events.py") "trans_novel\json_events.py"
    Copy-Item -Force (Join-Path $PSScriptRoot "test_json_events.py") "tests\test_json_events.py"
    Write-Host "Installed trans_novel\json_events.py and tests\test_json_events.py"
}
finally { Pop-Location }

Write-Host ""
Write-Host "Verify:"
Write-Host "  cd $RepoRoot"
Write-Host "  uv run --no-sync pytest -q tests/test_json_events.py"
Write-Host "  uv run --no-sync python -m trans_novel --help | Select-String json-events"

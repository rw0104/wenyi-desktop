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
# ErrorActionPreference=Stop, so merge streams and check the exit code ourselves.
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments, [string]$What)
    & $Exe @Arguments 2>&1 | ForEach-Object { Write-Host "  $_" }
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit code $LASTEXITCODE)" }
}

if (-not (Test-Path (Join-Path $RepoRoot "trans_novel\cli.py"))) {
    throw "Not a Wenyi checkout: $RepoRoot"
}

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

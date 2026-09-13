# End-to-end verification of the desktop pipeline, driven exactly as the app drives it.
#
# Exercises the real path -- sidecar -> engine -> HTTP -> translated book -- without a paid
# API key, by pointing the engine at scripts/mock_llm.py.
#
# This is the check that catches what compiling and unit tests cannot: argument ordering,
# which provider modules PyInstaller actually bundled, and whether state lands where the
# resume view looks for it.
#
# Usage:
#   .\scripts\verify_pipeline.ps1
#   .\scripts\verify_pipeline.ps1 -Sidecar ..\wenyi-engine\dist\wenyi-core.exe
param(
    [string]$Sidecar,
    [int]$Port = 18080
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if (-not $Sidecar) {
    $Sidecar = Join-Path $RepoRoot "src-tauri\binaries\wenyi-core-x86_64-pc-windows-msvc.exe"
}
if (-not (Test-Path $Sidecar)) {
    throw "Sidecar not found: $Sidecar`nBuild it first: .\sidecar\build_sidecar.ps1"
}

$Work = Join-Path $env:TEMP "wenyi-verify"
$Workspace = Join-Path $Work "workspace"
$Books = Join-Path $Work "books"

Write-Host "sidecar   : $Sidecar"
Write-Host "workspace : $Workspace"
Write-Host ""

Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Workspace, (Join-Path $Books "output") | Out-Null

@"
Chapter One

The wind rose over the harbour at dawn, and the boats began to stir.

She had never seen the sea so calm.

Chapter Two

He returned to the old house three winters later.
"@ | Set-Content -Encoding utf8 (Join-Path $Books "sample-book.txt")

# config.yaml as the app generates it for a custom OpenAI-compatible endpoint.
@"
language:
  source: "en"
  target: "zh"

llm:
  providers:
    custom:
      kind: openai-compatible
      base_url: "http://127.0.0.1:$Port/v1"
      api_key_env: "MOCK_API_KEY"
  models:
    custom_model:
      provider: custom
      model: "mock-model"
  tiers:
    strong: custom_model
    cheap: custom_model
    fast: custom_model

pipeline:
  review: false
  polish: false
  book_understanding: false

output:
  mono: true
  bilingual: false
  bilingual_order: target_first
  about_page: true
"@ | Set-Content -Encoding utf8 (Join-Path $Workspace "config.yaml")

# Start the mock LLM.
$mock = Start-Process -FilePath "python" `
    -ArgumentList (Join-Path $PSScriptRoot "mock_llm.py"), $Port `
    -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2

$stdoutFile = Join-Path $Work "stdout.jsonl"
$stderrFile = Join-Path $Work "stderr.log"
$code = 1

try {
    # The app injects the key as an environment variable, never on the command line.
    $env:MOCK_API_KEY = "dummy-key-for-local-mock"
    # The mock listens on loopback; a proxy would hijack the request.
    foreach ($name in @("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy")) {
        Remove-Item "Env:$name" -ErrorAction SilentlyContinue
    }

    Push-Location $Workspace
    try {
        # The engine writes human output to stderr. Windows PowerShell 5.1 turns native
        # stderr into a terminating error under ErrorActionPreference=Stop, so relax it
        # locally and read the exit code directly.
        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        # Group-level options (--config, --json-events) MUST precede the subcommand;
        # command-level flags follow it. This ordering is what the app must produce.
        & $Sidecar --config (Join-Path $Workspace "config.yaml") --json-events `
            translate (Join-Path $Books "sample-book.txt") `
            > $stdoutFile 2> $stderrFile
        $code = $LASTEXITCODE
        $ErrorActionPreference = $previous
    }
    finally { Pop-Location }
}
finally {
    Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue
}

Write-Host "=== sidecar exit code: $code ==="
Write-Host ""
Write-Host "=== stdout (must be pure JSONL) ==="
Get-Content $stdoutFile -ErrorAction SilentlyContinue

# Assertions.
$events = @()
foreach ($line in Get-Content $stdoutFile -ErrorAction SilentlyContinue) {
    if ($line.Trim()) { $events += ($line | ConvertFrom-Json) }
}
$failures = @()
if ($code -ne 0) { $failures += "sidecar exited with $code" }
if (-not ($events | Where-Object { $_.event -eq "done" })) { $failures += "no terminal 'done' event" }
$errors = $events | Where-Object { $_.event -eq "error" }
if ($errors) { $failures += "error event: $($errors[0].message)" }

$stateDir = Join-Path $Workspace "state"
if (-not (Test-Path $stateDir)) { $failures += "no state/ tree created under the workspace" }

$outputs = Get-ChildItem (Join-Path $Books "output") -ErrorAction SilentlyContinue
if (-not $outputs) { $failures += "no output file produced" }

Write-Host ""
Write-Host "=== state tree under the workspace ==="
if (Test-Path $stateDir) {
    Get-ChildItem -Recurse -File $stateDir |
        ForEach-Object { "  " + $_.FullName.Replace($Workspace, "") }
} else { "  (none)" }

Write-Host ""
Write-Host "=== produced output ==="
if ($outputs) {
    $outputs | ForEach-Object { "  $($_.Name)  ($([math]::Round($_.Length/1KB,1)) KB)" }
} else { "  (none)" }

Write-Host ""
if ($failures.Count) {
    Write-Host "FAILED:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "PASS: pipeline produced a translated book through the app's exact invocation." -ForegroundColor Green

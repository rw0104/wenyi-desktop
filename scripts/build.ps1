# One-shot build for Wenyi Desktop on Windows.
# 1) builds the Python sidecar, 2) runs `tauri build`, 3) collects installers.
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

$Root = $PSScriptRoot

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm is required. Install Node.js LTS first."
}

Write-Host "==> Building sidecar ..."
& (Join-Path $Root "sidecar\build_sidecar.ps1")

Push-Location $Root
try {
    if (-not (Test-Path "node_modules")) {
        Invoke-Native npm @("install") "npm install"
    }
    Write-Host "==> tauri build ..."
    Invoke-Native npm @("run", "tauri", "build") "tauri build"
}
finally { Pop-Location }

$Target = Join-Path $Root "src-tauri\target\release\bundle"
if (Test-Path $Target) {
    Write-Host "==> Installers:"
    Get-ChildItem -Recurse -File $Target |
        Where-Object { $_.Extension -in ".exe", ".msi" } |
        ForEach-Object { Write-Host "  $($_.FullName)" }
}

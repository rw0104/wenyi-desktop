# Build the wenyi-core sidecar (PyInstaller single-file) and drop it into
# src-tauri/binaries/ where Tauri v2 auto-detects and bundles it.
#
# Usage:
#   .\build_sidecar.ps1
#   .\build_sidecar.ps1 -RepoRoot D:\path\to\wenyi
#
# Requires: uv (https://docs.astral.sh/uv) and a wenyi source checkout.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\wenyi")).Path,
    [string]$TargetTriple = "x86_64-pc-windows-msvc"
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

if (-not (Test-Path (Join-Path $RepoRoot "trans_novel\__main__.py"))) {
    throw "Not a Wenyi checkout: $RepoRoot"
}

$OutDir = Join-Path $PSScriptRoot "..\src-tauri\binaries"
$VenvPython = Join-Path $RepoRoot ".venv\Scripts\python.exe"

Write-Host "Repo root: $RepoRoot"

Push-Location $RepoRoot
try {
    Write-Host "[1/3] uv sync ..."
    Invoke-Native uv @("sync", "--locked") "uv sync"

    Write-Host "[2/3] installing PyInstaller ..."
    if (Test-Path $VenvPython) {
        Invoke-Native uv @("pip", "install", "pyinstaller", "--python", $VenvPython) "pyinstaller install"
    } else {
        Invoke-Native uv @("pip", "install", "pyinstaller") "pyinstaller install"
    }

    Write-Host "[3/3] PyInstaller build ..."
    Invoke-Native uv @(
        "run", "--no-sync", "python", "-m", "PyInstaller",
        "--name", "wenyi-core",
        "--onefile", "--clean", "--noconfirm",
        "--paths", ".",
        "--collect-all", "ebooklib", "--collect-all", "bs4", "--collect-all", "lxml",
        "--collect-all", "openai", "--collect-all", "pydantic", "--collect-all", "yaml",
        "--collect-data", "trans_novel.i18n",
        "--copy-metadata", "trans-novel",
        "--workpath", "build/pyinstaller",
        "--specpath", "build/pyinstaller-spec",
        "--distpath", "dist",
        "trans_novel/__main__.py"
    ) "PyInstaller"

    # Tauri's externalBin expects the target-triple suffix on the binary name.
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    $src = Join-Path $RepoRoot "dist\wenyi-core.exe"
    Copy-Item -Force $src (Join-Path $OutDir "wenyi-core-$TargetTriple.exe")
}
finally { Pop-Location }

Write-Host ""
Write-Host "Done. Sidecar written to $OutDir"
Write-Host "Next: npm run tauri build  (from wenyi-desktop\)"

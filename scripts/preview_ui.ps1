# Render the real UI to PNG files for design review.
#
# The frontend only depends on `window.__TAURI__.core.invoke` and `...event.listen`, so
# stubbing those (scripts/ui_preview_shim.js) renders the actual markup and CSS in a plain
# browser -- no build, no running application, no API key.
#
# Captures both colour schemes. Chrome/Edge report `prefers-color-scheme: light` by
# default in headless mode; `--force-dark-mode` switches the media query to dark.
#
# Usage:
#   .\scripts\preview_ui.ps1
#   .\scripts\preview_ui.ps1 -OutDir D:\shots -Width 1280 -Height 900
param(
    [string]$OutDir,
    [int]$Width = 1180,
    [int]$Height = 880,
    [string]$Browser
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$UiDir = Join-Path $RepoRoot "ui"
if (-not $OutDir) { $OutDir = Join-Path $env:TEMP "wenyi-ui-preview" }

if (-not $Browser) {
    foreach ($candidate in @(
        "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        "C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "C:\Program Files\Google\Chrome\Application\chrome.exe",
        "C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"
    )) {
        if (Test-Path $candidate) { $Browser = $candidate; break }
    }
}
if (-not $Browser -or -not (Test-Path $Browser)) {
    throw "No Chromium-based browser found for headless screenshots. Pass -Browser <path>."
}

# 1. Stage a copy of the UI plus the shim, so nothing here touches the shipped files.
$Stage = Join-Path $env:TEMP "wenyi-ui-stage"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage, $OutDir | Out-Null
Copy-Item (Join-Path $UiDir "*") $Stage -Recurse -Force

$shim = Get-Content (Join-Path $PSScriptRoot "ui_preview_shim.js") -Raw
$html = Get-Content (Join-Path $Stage "index.html") -Raw
# The shim must run before main.js, which captures window.__TAURI__ at load time.
$html = $html.Replace(
    '<script src="main.js"></script>',
    "<script>`n$shim`n</script>`n    <script src=`"main.js`"></script>"
)
[System.IO.File]::WriteAllText(
    (Join-Path $Stage "index.html"), $html, (New-Object System.Text.UTF8Encoding($false))
)

$page = "file:///" + ((Join-Path $Stage "index.html") -replace '\\', '/')

function Invoke-Shot {
    param([string]$Name, [string[]]$ExtraArgs, [string]$Profile)
    $shot = Join-Path $OutDir "$Name.png"
    Remove-Item $shot -Force -ErrorAction SilentlyContinue
    $args = @(
        "--headless=new",
        "--disable-gpu",
        "--hide-scrollbars",
        "--no-first-run",
        "--no-default-browser-check",
        "--user-data-dir=$Profile",
        "--window-size=$Width,$Height",
        "--virtual-time-budget=2500",
        "--screenshot=$shot"
    ) + $ExtraArgs + @($page)
    # Chromium logs warnings to stderr; Windows PowerShell 5.1 turns native stderr into a
    # terminating error under ErrorActionPreference=Stop, so relax it locally.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    & $Browser @args 2>&1 | Out-Null
    $ErrorActionPreference = $previous
    if (-not (Test-Path $shot)) { throw "Screenshot failed: $Name" }
    return $shot
}

Write-Host "browser : $Browser"
Write-Host "page    : $page"
Write-Host "out     : $OutDir"
Write-Host ""

$light = Invoke-Shot -Name "wenyi-light" -ExtraArgs @() `
    -Profile (Join-Path $env:TEMP "wenyi-shot-light")
$dark = Invoke-Shot -Name "wenyi-dark" -ExtraArgs @("--force-dark-mode") `
    -Profile (Join-Path $env:TEMP "wenyi-shot-dark")

# Report the sampled background pixel so the colour scheme is verifiable, not assumed.
Add-Type -AssemblyName System.Drawing
foreach ($shot in @($light, $dark)) {
    $bmp = New-Object System.Drawing.Bitmap($shot)
    $pixel = $bmp.GetPixel(8, $Height - 8)  # bottom-left empty area = page background
    $bmp.Dispose()
    Write-Host ("{0}: {1}x{2}  bg=#{3:X2}{4:X2}{5:X2}" -f `
        (Split-Path $shot -Leaf), $Width, $Height, $pixel.R, $pixel.G, $pixel.B)
}

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

# Read and write as explicit UTF-8. Windows PowerShell 5.1 reads a BOM-less file as the
# system ANSI codepage, which silently turns every Chinese string in the UI into mojibake --
# and the screenshots then document a broken interface rather than the real one.
$Utf8 = New-Object System.Text.UTF8Encoding($false)
$shim = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "ui_preview_shim.js"), [System.Text.Encoding]::UTF8)
$html = [System.IO.File]::ReadAllText((Join-Path $Stage "index.html"), [System.Text.Encoding]::UTF8)
# The shim must run before main.js, which captures window.__TAURI__ at load time.
$html = $html.Replace(
    '<script src="main.js"></script>',
    "<script>`n$shim`n</script>`n    <script src=`"main.js`"></script>"
)
$staged = Join-Path $Stage "index.html"
[System.IO.File]::WriteAllText($staged, $html, $Utf8)

# Guard the guard: verify the staged page survived the encoding round trip before rendering.
# A preview that documents mojibake is worse than no preview, because it looks authoritative.
# Samples are taken at runtime from the source page, which keeps this script pure ASCII --
# PowerShell 5.1 reads a BOM-less .ps1 as the ANSI codepage, so non-ASCII here would corrupt
# the checker itself. They are also taken per panel: comparing the settings screen against
# text that only exists on the translate screen proves nothing either way.
$check = [System.IO.File]::ReadAllText($staged, [System.Text.Encoding]::UTF8)
$source = [System.IO.File]::ReadAllText((Join-Path $UiDir "index.html"), [System.Text.Encoding]::UTF8)

function Get-PanelSamples {
    param([string]$Html, [string]$Tab, [int]$Count = 6)
    $marker = 'id="panel-' + $Tab + '"'
    $from = $Html.IndexOf($marker)
    if ($from -lt 0) { return @() }
    $rest = $Html.Substring($from)
    $next = $rest.IndexOf('id="panel-', 1)
    if ($next -gt 0) { $rest = $rest.Substring(0, $next) }
    return @([regex]::Matches($rest, '>([^<>\x00-\x7F]{3,16})<') |
        ForEach-Object { $_.Groups[1].Value.Trim() } |
        Where-Object { $_.Length -ge 3 } |
        Select-Object -Unique -First $Count)
}

$allSamples = Get-PanelSamples -Html $source -Tab "translate"
if ($allSamples.Count -eq 0) { throw "No non-ASCII sample found in the source page." }
# Every sample must survive into the staged page, for every panel the preview can show.
foreach ($tab in @("translate", "settings", "resume")) {
    $panelSamples = Get-PanelSamples -Html $source -Tab $tab
    $lost = @($panelSamples | Where-Object { -not $check.Contains($_) })
    if ($lost.Count -gt 0) {
        throw "Staged page lost $($lost.Count) sample(s) from the $tab panel (encoding round trip failed)."
    }
}
Write-Host "staged page verified: all panel text survived staging"

$page = "file:///" + ((Join-Path $Stage "index.html") -replace '\\', '/')

function Invoke-Shot {
    param([string]$Name, [string[]]$ExtraArgs, [string]$Tab = "translate")
    $shot = Join-Path $OutDir "$Name.png"
    Remove-Item $shot -Force -ErrorAction SilentlyContinue

    # A fresh profile per shot, deleted first. Chromium caches by URL, and the staged page
    # keeps the same file:// URL between runs only by accident of naming -- when it did, every
    # screenshot after the first silently returned the first cached render, so the tool
    # documented a stale page while the source on disk was correct.
    $profile = Join-Path $env:TEMP "wenyi-shot-$Name-$Nonce"
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue

    $args = @(
        "--headless=new",
        "--disable-gpu",
        "--hide-scrollbars",
        "--no-first-run",
        "--no-default-browser-check",
        "--user-data-dir=$profile",
        "--disk-cache-size=1",
        "--window-size=$Width,$Height",
        "--virtual-time-budget=2500",
        "--screenshot=$shot"
    ) + $ExtraArgs + @("$page#$Tab")
    # Chromium logs warnings to stderr; Windows PowerShell 5.1 turns native stderr into a
    # terminating error under ErrorActionPreference=Stop, so relax it locally.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    & $Browser @args 2>&1 | Out-Null
    $ErrorActionPreference = $previous
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
    if (-not (Test-Path $shot)) { throw "Screenshot failed: $Name" }
    return $shot
}

# Verify a produced image by reading its text back. Catches what inspecting the source
# cannot: a cached or stale render, a missing font, a page that never executed.
#
# Two kinds of evidence, because OCR is not a faithful transcriber:
#   - ASCII markers must appear verbatim; OCR reads Latin text reliably
#   - text samples are compared with whitespace removed, since the engine separates CJK
#     glyphs, and only a majority is required because single glyphs are sometimes misread
function Assert-ShotText {
    param([string]$Shot, [string[]]$Samples, [string[]]$AsciiMarkers, [string]$Label)
    $ocr = Join-Path $PSScriptRoot "ocr_text.ps1"
    if (-not (Test-Path $ocr)) { Write-Warning "ocr_text.ps1 missing; skipping image check"; return }
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $raw = & $ocr -Path $Shot 2>&1 | Out-String
    $ErrorActionPreference = $previous

    $flat = ($raw -replace '\s', '')
    $missing = @($AsciiMarkers | Where-Object { -not $flat.Contains($_) })
    if ($missing.Count -gt 0) {
        throw "$Label image is missing markers: $($missing -join ', '). The render is stale or broken."
    }

    # Compare by character overlap rather than exact containment. OCR misreads individual
    # glyphs and the engine separates CJK characters, so exact matching is brittle; but text
    # turned to mojibake shares almost no characters with the source, while a merely
    # imperfect reading shares most of them.
    $best = 0.0
    foreach ($sample in $Samples) {
        $chars = @($sample.ToCharArray() | Where-Object { [int]$_ -gt 127 })
        if ($chars.Count -eq 0) { continue }
        $present = @($chars | Where-Object { $flat.Contains($_) }).Count
        $ratio = $present / $chars.Count
        if ($ratio -gt $best) { $best = $ratio }
    }
    if ($best -lt 0.6) {
        throw ("$Label image shares only {0:P0} of its characters with the source text. " -f $best) +
            "The render is stale or broken."
    }
    Write-Host ("{0}: {1} markers + {2:P0} character overlap" -f `
        $Label, $AsciiMarkers.Count, $best)
}

Write-Host "browser : $Browser"
Write-Host "page    : $page"
Write-Host "out     : $OutDir"
Write-Host ""

$light = Invoke-Shot -Name "wenyi-light" -Tab "translate"
$dark = Invoke-Shot -Name "wenyi-dark" -Tab "translate" -ExtraArgs @("--force-dark-mode")
$settings = Invoke-Shot -Name "wenyi-settings" -Tab "settings"

# Read the images back so a stale or broken render cannot be published silently. The ASCII
# markers identify current content: a cached older render would not contain them.
Assert-ShotText -Shot $light -Label "shelf (light)" `
    -Samples (Get-PanelSamples -Html $source -Tab "translate") `
    -AsciiMarkers @("TheEconomist", "MiddleEast")
Assert-ShotText -Shot $settings -Label "settings" `
    -Samples (Get-PanelSamples -Html $source -Tab "settings") `
    -AsciiMarkers @("tokenrhythm", "deepseek-v4-pro-0813")

# Report the sampled background pixel so the colour scheme is verifiable, not assumed.
Add-Type -AssemblyName System.Drawing
foreach ($shot in @($light, $dark, $settings)) {
    $bmp = New-Object System.Drawing.Bitmap($shot)
    $pixel = $bmp.GetPixel(8, $Height - 8)  # bottom-left empty area = page background
    $bmp.Dispose()
    Write-Host ("{0}: {1}x{2}  bg=#{3:X2}{4:X2}{5:X2}" -f `
        (Split-Path $shot -Leaf), $Width, $Height, $pixel.R, $pixel.G, $pixel.B)
}

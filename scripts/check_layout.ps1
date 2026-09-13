# Check that the layout adapts to the window sizes and text sizes the app actually sees.
#
# Renders the real interface at several window sizes and several user text sizes, and reads a
# probe the page writes into its own DOM, reporting horizontal overflow and any element that
# extends past the viewport. A screenshot shows whether one size looks right; this says whether
# the layout fits at every size, which is the part that cannot be judged by looking.
#
# The text-size axis is the accessibility half of the same question: layout is in rem, so a
# user who raises their system text size should get a larger interface, not a broken one.
# Raising the root font size here is equivalent to the browser's own default-font-size setting,
# because the stylesheet leaves the root at `font-size: 100%` instead of pinning it to px.
param(
    [string[]]$Sizes = @("760x560", "900x700", "1080x720", "1600x1000", "2200x1200"),
    [int[]]$TextScale = @(100, 125, 150),
    # Every panel, because they hold different content: settings has the most controls and
    # the shelf has the widest single line, so a size that fits one can still break another.
    [string[]]$Tab = @("translate", "resume", "settings"),
    [string]$Browser
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$UiDir = Join-Path $RepoRoot "ui"

if (-not $Browser) {
    foreach ($candidate in @(
        "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        "C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "C:\Program Files\Google\Chrome\Application\chrome.exe"
    )) {
        if (Test-Path $candidate) { $Browser = $candidate; break }
    }
}
if (-not $Browser) { throw "No Chromium-based browser found." }

$Utf8 = New-Object System.Text.UTF8Encoding($false)
$Nonce = [guid]::NewGuid().ToString("N").Substring(0, 8)
$Stage = Join-Path $env:TEMP "wenyi-layout-$Nonce"
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $UiDir "*") $Stage -Recurse -Force

$shim = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "ui_preview_shim.js"), [System.Text.Encoding]::UTF8)
$html = [System.IO.File]::ReadAllText((Join-Path $Stage "index.html"), [System.Text.Encoding]::UTF8)
$html = $html.Replace('<script src="main.js"></script>', "<script>`n$shim`n</script>`n    <script src=`"main.js`"></script>")

Write-Host "browser: $Browser"
Write-Host ""

$failures = @()
$checked = 0

foreach ($tab in $Tab) {
  Write-Host "tab: $tab" -ForegroundColor Yellow
  foreach ($scale in $TextScale) {
    # The override sits after the stylesheet in document order, so it wins the cascade at equal
    # specificity - the same lever the browser's text-size setting pulls.
    $scaled = if ($scale -eq 100) {
        $html
    } else {
        $html.Replace("</head>", "<style>html{font-size:$scale%}</style>`n</head>")
    }
    $staged = Join-Path $Stage "index-$scale.html"
    [System.IO.File]::WriteAllText($staged, $scaled, $Utf8)
    $page = "file:///" + ($staged -replace '\\', '/')

    Write-Host "  text size ${scale}%"
    Write-Host ("  {0,-11} {1,-10} {2,5} {3,7} {4,7} {5,8} {6,7} {7,6}  {8}" -f `
        "requested", "viewport", "root", "ovfl", "clip", "chrome", "cover", "cols", "verdict")
    Write-Host ("  " + "-" * 86)

    foreach ($size in $Sizes) {
        $parts = $size.Split("x")
        $w = [int]$parts[0]; $h = [int]$parts[1]

        $profile = Join-Path $env:TEMP "wenyi-layout-p-$Nonce-$tab-$scale-$w"
        Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
        $out = Join-Path $Stage "dom-$tab-$scale-$w.html"

        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        Start-Process -FilePath $Browser -NoNewWindow -Wait -PassThru `
            -RedirectStandardOutput $out -RedirectStandardError (Join-Path $Stage "err-$tab-$scale-$w.txt") `
            -ArgumentList @(
                "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
                "--hide-scrollbars", "--user-data-dir=$profile", "--disk-cache-size=1",
                "--window-size=$w,$h", "--virtual-time-budget=3000",
                "--dump-dom", "$page#$tab"
            ) | Out-Null
        $ErrorActionPreference = $previous
        Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue

        $dom = [System.IO.File]::ReadAllText($out, [System.Text.Encoding]::UTF8)
        $match = [regex]::Match($dom, 'id="layout-probe"[^>]*>([^<]*)<')
        if (-not $match.Success) {
            Write-Host ("  {0,-11} probe missing" -f $size)
            $failures += "$tab ${scale}% $size : probe not found"
            continue
        }
        $probe = $match.Groups[1].Value | ConvertFrom-Json
        $checked++

        # The probe must show the text actually got bigger, otherwise a green row would only
        # prove the override never applied.
        $expectedRoot = [Math]::Round(16 * $scale / 100, 1)
        $scaled_ok = [Math]::Abs($probe.rootFontSize - $expectedRoot) -lt 1.5

        # A viewport narrower than requested means the browser clamped the window, not that the
        # layout failed; the real constraint is that content must fit the viewport it got.
        $verdict = if ($probe.overflowPx -le 1 -and $probe.clippedCount -eq 0 -and $scaled_ok) { "ok" } else { "OVERFLOW" }
        if ($verdict -ne "ok") {
            $why = if (-not $scaled_ok) { "text scale did not apply (root $($probe.rootFontSize)px)" }
                   else { "overflow $($probe.overflowPx)px, clipped: $($probe.clipped -join ', ')" }
            $failures += "$tab ${scale}% $size : $why"
        }
        # "-" rather than 0: the shelf lives on one panel only, so on the others these are
        # absent, not measured-as-nothing.
        $coverText = if ($null -eq $probe.coverWidth) { "-" } else { "$($probe.coverWidth)" }
        $colsText = if ($null -eq $probe.shelfColumns) { "-" } else { "$($probe.shelfColumns)" }
        Write-Host ("  {0,-11} {1,-10} {2,5} {3,5}px {4,7} {5,8} {6,7} {7,6}  {8}" -f `
            $size, $probe.viewport, $probe.rootFontSize, $probe.overflowPx, $probe.clippedCount,
            $probe.chromeDirection, $coverText, $colsText, $verdict)
    }
    Write-Host ""
  }
}

if ($failures.Count -gt 0) {
    Write-Host "FAILED:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "PASS: $checked renders fit - every panel, at every window size and text size." -ForegroundColor Green

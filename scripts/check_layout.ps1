# Check that the layout adapts to the window sizes and text sizes the app actually sees.
#
# Renders the real interface at several window sizes, several user text sizes, and both shelf
# states, and reads a probe the page writes into its own DOM, reporting horizontal overflow and
# any element that extends past the viewport. A screenshot shows whether one size looks right;
# this says whether the layout fits at every size, which is the part that cannot be judged by
# looking.
#
# The text-size axis is the accessibility half of the same question: layout is in rem, so a
# user who raises their system text size should get a larger interface, not a broken one.
# Raising the root font size here is equivalent to the browser's own default-font-size setting,
# because the stylesheet leaves the root at `font-size: 100%` instead of pinning it to px.
#
# Loop variables are deliberately named differently from the parameters they walk. PowerShell
# variable names are case-insensitive, so `foreach ($tab in $Tab)` assigns into the collection
# being iterated: after the first pass $Tab collapses to a single string and every later pass
# silently runs one value instead of all of them. That cost this script two thirds of its
# coverage while still printing PASS.
param(
    [string[]]$Sizes = @("760x560", "900x700", "1080x720", "1600x1000", "2200x1200"),
    [int[]]$TextScale = @(100, 125, 150),
    # Every panel, because they hold different content: settings has the most controls and
    # the shelf has the widest single line, so a size that fits one can still break another.
    [string[]]$Tab = @("translate", "resume", "settings"),
    # One book and several, because `.shelf.single` is a different layout entirely - a cover
    # beside its metadata instead of a grid of covers. Checking only the populated shelf left
    # the single-book case untested, which is the state a first-time user is actually in.
    [int[]]$Books = @(3, 1),
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

$shim0 = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "ui_preview_shim.js"), [System.Text.Encoding]::UTF8)
$html0 = [System.IO.File]::ReadAllText((Join-Path $Stage "index.html"), [System.Text.Encoding]::UTF8)

Write-Host "browser: $Browser"
Write-Host ""

$failures = @()
$checked = 0

foreach ($bookCount in $Books) {
    $shim = $shim0.Replace('case "list_library":', "case `"list_library`":`n            return shelfBooks.slice(0, $bookCount).map(({ cover, ...book }) => book);")
    $shim = $shim.Replace('case "add_books":', "case `"add_books`":`n            return shelfBooks.slice(0, $bookCount).map(({ cover, ...book }) => book);")
    $html = $html0.Replace('<script src="main.js"></script>', "<script>`n$shim`n</script>`n    <script src=`"main.js`"></script>")

    foreach ($tabName in $Tab) {
        Write-Host "tab: $tabName    shelf: $bookCount book(s)" -ForegroundColor Yellow

        foreach ($scalePct in $TextScale) {
            # The override sits after the stylesheet in document order, so it wins the cascade at
            # equal specificity - the same lever the browser's text-size setting pulls.
            $scaled = if ($scalePct -eq 100) {
                $html
            } else {
                $html.Replace("</head>", "<style>html{font-size:$scalePct%}</style>`n</head>")
            }
            $staged = Join-Path $Stage ("index-{0}-{1}.html" -f $bookCount, $scalePct)
            [System.IO.File]::WriteAllText($staged, $scaled, $Utf8)
            $page = "file:///" + ($staged -replace '\\', '/')

            Write-Host "  text size ${scalePct}%"
            Write-Host ("  {0,-11} {1,-10} {2,5} {3,7} {4,7} {5,8} {6,7} {7,6}  {8}" -f `
                "requested", "viewport", "root", "ovfl", "clip", "chrome", "cover", "cols", "verdict")
            Write-Host ("  " + "-" * 86)

            foreach ($sizeSpec in $Sizes) {
                $dims = $sizeSpec.Split("x")
                $winW = [int]$dims[0]; $winH = [int]$dims[1]

                $profile = Join-Path $env:TEMP ("wenyi-layout-p-{0}-{1}-{2}-{3}-{4}" -f $Nonce, $bookCount, $tabName, $scalePct, $winW)
                Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue
                $domFile = Join-Path $Stage ("dom-{0}-{1}-{2}-{3}.html" -f $bookCount, $tabName, $scalePct, $winW)

                $previous = $ErrorActionPreference
                $ErrorActionPreference = "Continue"
                Start-Process -FilePath $Browser -NoNewWindow -Wait -PassThru `
                    -RedirectStandardOutput $domFile -RedirectStandardError (Join-Path $Stage "err.txt") `
                    -ArgumentList @(
                        "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
                        "--hide-scrollbars", "--user-data-dir=$profile", "--disk-cache-size=1",
                        "--window-size=$winW,$winH", "--virtual-time-budget=3000",
                        "--dump-dom", "$page#$tabName"
                    ) | Out-Null
                $ErrorActionPreference = $previous
                Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue

                $dom = [System.IO.File]::ReadAllText($domFile, [System.Text.Encoding]::UTF8)
                $found = [regex]::Match($dom, 'id="layout-probe"[^>]*>([^<]*)<')
                if (-not $found.Success) {
                    Write-Host ("  {0,-11} probe missing" -f $sizeSpec)
                    $failures += "$bookCount book(s), $tabName ${scalePct}% $sizeSpec : probe not found"
                    continue
                }
                $probe = $found.Groups[1].Value | ConvertFrom-Json
                $checked++

                # The probe must show the text actually got bigger, otherwise a green row would
                # only prove the override never applied.
                $expectedRoot = [Math]::Round(16 * $scalePct / 100, 1)
                $scaleApplied = [Math]::Abs($probe.rootFontSize - $expectedRoot) -lt 1.5

                # A viewport narrower than requested means the browser clamped the window, not
                # that the layout failed; content must fit the viewport it actually got.
                $verdict = if ($probe.overflowPx -le 1 -and $probe.clippedCount -eq 0 -and $scaleApplied) { "ok" } else { "OVERFLOW" }
                if ($verdict -ne "ok") {
                    $why = if (-not $scaleApplied) { "text scale did not apply (root $($probe.rootFontSize)px)" }
                           else { "overflow $($probe.overflowPx)px, clipped: $($probe.clipped -join ', ')" }
                    $failures += "$bookCount book(s), $tabName ${scalePct}% $sizeSpec : $why"
                }

                # "-" rather than 0: the shelf lives on one panel only, so on the others these
                # are absent, not measured-as-nothing.
                $coverText = if ($null -eq $probe.coverWidth) { "-" } else { "$($probe.coverWidth)" }
                $colsText = if ($null -eq $probe.shelfColumns) { "-" } else { "$($probe.shelfColumns)" }
                Write-Host ("  {0,-11} {1,-10} {2,5} {3,5}px {4,7} {5,8} {6,7} {7,6}  {8}" -f `
                    $sizeSpec, $probe.viewport, $probe.rootFontSize, $probe.overflowPx, $probe.clippedCount,
                    $probe.chromeDirection, $coverText, $colsText, $verdict)
            }
            Write-Host ""
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host "FAILED:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "PASS: $checked renders fit - every panel, at every window size and text size." -ForegroundColor Green

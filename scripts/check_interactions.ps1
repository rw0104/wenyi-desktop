# Behavioural checks for the shelf and run controls.
#
# `check_layout.ps1` answers "does it fit". This answers "does it do the right thing" - the
# class of bug that renders perfectly and still misbehaves. Each case drives the real UI in a
# headless browser and reads a verdict the page writes into its own DOM.
#
# Every case here corresponds to a bug a user actually reported, and each was confirmed to
# FAIL against the old behaviour before the fix landed. A case that cannot fail is decoration.
param([string]$Browser)

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

# The message under test, written as JS escapes rather than literal CJK. PowerShell 5.1 reads a
# BOM-less .ps1 as ANSI, so a Chinese literal here would be mangled before the browser ever saw
# it - the check would then "pass" by searching for the wrong string. This file stays pure ASCII.
$refused = '\u4e0d\u80fd\u5207\u6362\u4e66\u7c4d'   # "cannot switch books"

# Each case: a name, how many books the shelf holds, and the script that drives the page and
# writes `#case-probe` with a JSON verdict carrying `ok` plus whatever evidence it gathered.
$cases = @(
    @{
        Name  = "clicking the only book during a run is not an error"
        Books = 1
        Drive = @"
    const book = document.querySelector(".book");
    if (book) book.click();
    // A shelf re-render used to call the running guard for its side effect, writing the same
    // bogus error with no click involved. Drive one so both paths are covered.
    if (typeof SHELF === "object" && SHELF.set) SHELF.set([]);
    const text = document.getElementById("log").textContent;
    verdict = {
      ok: !text.includes("$refused"),
      clicked: !!book,
      hits: (text.match(/$refused/g) || []).length,
    };
"@
    },
    @{
        Name  = "clicking the already-selected book during a run is not an error"
        Books = 3
        Drive = @"
    const books = [...document.querySelectorAll(".book")];
    if (books[0]) books[0].click();          // select the first
    if (books[0]) books[0].click();          // and click it again while running
    const text = document.getElementById("log").textContent;
    verdict = {
      ok: !text.includes("$refused"),
      books: books.length,
      hits: (text.match(/$refused/g) || []).length,
    };
"@
    },
    @{
        Name  = "clicking a different book during a run is still refused"
        Books = 3
        Drive = @"
    const books = [...document.querySelectorAll(".book")];
    if (books[0]) books[0].click();
    if (books[1]) books[1].click();          // a genuine switch, must be refused
    const text = document.getElementById("log").textContent;
    verdict = {
      ok: text.includes("$refused"),
      books: books.length,
      hits: (text.match(/$refused/g) || []).length,
    };
"@
    }
)

Write-Host "browser: $Browser"
Write-Host ""
Write-Host ("{0,-56} {1}" -f "case", "result")
Write-Host ("-" * 76)

$failures = @()
foreach ($case in $cases) {
    $nonce = [guid]::NewGuid().ToString("N").Substring(0, 8)
    $stage = Join-Path $env:TEMP "wenyi-behaviour-$nonce"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item (Join-Path $UiDir "*") $stage -Recurse -Force

    $shim = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "ui_preview_shim.js"), [System.Text.Encoding]::UTF8)
    $shim = $shim.Replace('case "list_library":', "case `"list_library`":`n            return shelfBooks.slice(0, $($case.Books)).map(({ cover, ...book }) => book);")
    $shim = $shim.Replace('case "add_books":', "case `"add_books`":`n            return shelfBooks.slice(0, $($case.Books)).map(({ cover, ...book }) => book);")
    $shim = $shim.Replace('document.body.append(probe);', 'window.__probe = probe;')

    $driver = @"

// Wait for the shim to finish populating the shelf and enter the running state.
window.addEventListener("load", () => {
  setTimeout(() => {
    let verdict = { ok: false, reason: "driver did not run" };
    try {
$($case.Drive)
    } catch (error) {
      verdict = { ok: false, reason: String(error) };
    }
    const d = document.createElement("div");
    d.id = "case-probe";
    d.textContent = JSON.stringify(verdict);
    document.body.append(d);
  }, 900);
});
"@

    $html = [System.IO.File]::ReadAllText((Join-Path $stage "index.html"), [System.Text.Encoding]::UTF8)
    $html = $html.Replace('<script src="main.js"></script>', "<script>`n$shim`n</script>`n    <script src=`"main.js`"></script>")
    $html = $html.Replace("</body>", "<script>$driver</script>`n</body>")
    [System.IO.File]::WriteAllText((Join-Path $stage "index.html"), $html, $Utf8)

    $page = "file:///" + ((Join-Path $stage "index.html") -replace '\\', '/')
    $profile = Join-Path $env:TEMP "wenyi-behaviour-p-$nonce"
    $domFile = Join-Path $stage "dom.html"

    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    Start-Process -FilePath $Browser -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $domFile -RedirectStandardError (Join-Path $stage "err.txt") `
        -ArgumentList @(
            "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
            "--hide-scrollbars", "--user-data-dir=$profile", "--disk-cache-size=1",
            "--window-size=1451,1400", "--virtual-time-budget=6000",
            "--dump-dom", "$page#translate"
        ) | Out-Null
    $ErrorActionPreference = $previous
    Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue

    $dom = [System.IO.File]::ReadAllText($domFile, [System.Text.Encoding]::UTF8)
    $found = [regex]::Match($dom, 'id="case-probe"[^>]*>([^<]*)<')
    if (-not $found.Success) {
        Write-Host ("{0,-56} probe missing" -f $case.Name)
        $failures += "$($case.Name) : probe not found"
        continue
    }
    $verdict = $found.Groups[1].Value | ConvertFrom-Json
    $evidence = ($verdict.PSObject.Properties |
        Where-Object { $_.Name -ne "ok" } |
        ForEach-Object { "$($_.Name)=$($_.Value)" }) -join "  "
    $result = if ($verdict.ok) { "ok" } else { "FAIL" }
    if (-not $verdict.ok) { $failures += "$($case.Name) : $evidence" }
    Write-Host ("{0,-56} {1,-6} {2}" -f $case.Name, $result, $evidence)
}

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host "FAILED:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "PASS: every interaction behaves as intended." -ForegroundColor Green

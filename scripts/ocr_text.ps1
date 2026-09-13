# Read the text out of a rendered screenshot so image content can be verified without
# looking at it. Uses the OCR engine built into Windows; nothing is installed.
#
# This exists because the preview pipeline silently produced screenshots of a mojibake
# interface for several rounds: the tooling was wrong while the application was fine, and
# nobody could tell from the source alone.
param(
    [Parameter(Mandatory = $true)][string[]]$Path,
    [string]$Language = "zh-Hans-CN"
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Runtime.WindowsRuntime | Out-Null
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and
        $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    })[0]

function Await($operation, $resultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($resultType)
    $netTask = $asTask.Invoke($null, @($operation))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}

[Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime] | Out-Null
[Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType = WindowsRuntime] | Out-Null
[Windows.Media.Ocr.OcrEngine, Windows.Media.Ocr, ContentType = WindowsRuntime] | Out-Null
[Windows.Globalization.Language, Windows.Globalization, ContentType = WindowsRuntime] | Out-Null

$engine = $null
try {
    $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage(
        (New-Object Windows.Globalization.Language $Language))
} catch { }
if (-not $engine) {
    $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
}
if (-not $engine) { throw "No OCR engine available." }
Write-Host "ocr language: $($engine.RecognizerLanguage.LanguageTag)"

foreach ($item in $Path) {
    $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($item)) ([Windows.Storage.StorageFile])
    $stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
    $decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
    $bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
    $result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])

    # The recognised text goes to the success stream so callers can capture it; only the
    # framing goes to the host. Write-Host alone would leave a pipeline caller with nothing.
    Write-Host ""
    Write-Host "===== $(Split-Path $item -Leaf) ====="
    Write-Output (($result.Lines | ForEach-Object { $_.Text }) -join " | ")
    $stream.Dispose()
}

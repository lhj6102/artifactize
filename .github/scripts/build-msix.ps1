# Build an unsigned Store upload from the same two executables as the zip.
param(
    [Parameter(Mandatory = $true)][string] $Version,
    [Parameter(Mandatory = $true)][string] $Dist,
    [string] $Output = 'dist'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$') { throw 'Invalid version' }
$parts = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
if (@($parts | Where-Object { $_ -gt 65535 }).Count -gt 0) { throw 'MSIX version components must fit UInt16' }
# Store requires revision zero. Prerelease packages are build/test artifacts only.
$msixVersion = "$($parts[0]).$($parts[1]).$($parts[2]).0"
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$Dist = (Resolve-Path $Dist).Path
New-Item -ItemType Directory -Path $Output -Force | Out-Null
$Output = (Resolve-Path $Output).Path
$root = Join-Path $env:TEMP ('artifactize-msix-' + [Guid]::NewGuid().ToString('N'))
$unpack = Join-Path $root 'unpack'
$stage = Join-Path $root 'stage'
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    $name = "artifactize-v$Version-x86_64-pc-windows-msvc"
    Expand-Archive -LiteralPath (Join-Path $Dist "$name.zip") -DestinationPath $unpack
    foreach ($command in @('artifactize', 'artifactize-tools')) {
        Copy-Item (Join-Path $unpack "$name\$command.exe") $stage
    }
    Copy-Item (Join-Path $repo 'LICENSE') $stage
    $manifest = [IO.File]::ReadAllText((Join-Path $repo '.github/packaging/AppxManifest.xml'))
    [IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest.Replace('@VERSION@', $msixVersion))
    # Use the existing raster brand mark; S8 can replace these visual assets.
    Add-Type -AssemblyName System.Drawing
    $assets = Join-Path $stage 'Assets'
    New-Item -ItemType Directory -Path $assets | Out-Null
    $source = [Drawing.Image]::FromFile((Join-Path $repo 'assets/brand/apple-touch-icon.png'))
    try {
        foreach ($logo in @(@('StoreLogo', 50), @('Square44x44Logo', 44), @('Square150x150Logo', 150))) {
            $bitmap = New-Object Drawing.Bitmap ([int]$logo[1]), ([int]$logo[1])
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.DrawImage($source, 0, 0, [int]$logo[1], [int]$logo[1])
                $bitmap.Save((Join-Path $assets "$($logo[0]).png"), [Drawing.Imaging.ImageFormat]::Png)
            } finally { $graphics.Dispose(); $bitmap.Dispose() }
        }
    } finally { $source.Dispose() }
    $sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $sdk) { throw 'Windows SDK makeappx.exe is required' }
    $package = Join-Path $Output "artifactize-v$Version-x64.msix"
    & $sdk.FullName pack /d $stage /p $package /o
    if ($LASTEXITCODE -ne 0) { throw 'makeappx validation/pack failed' }
    # .msixupload is a zip containing the unsigned .msix; Store signs it after ingestion.
    $upload = Join-Path $Output "artifactize-v$Version-x64.msixupload"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    if (Test-Path $upload) { Remove-Item $upload }
    $zip = [IO.Compression.ZipFile]::Open($upload, [IO.Compression.ZipArchiveMode]::Create)
    try { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $package, [IO.Path]::GetFileName($package)) | Out-Null }
    finally { $zip.Dispose() }
    foreach ($file in @($package, $upload)) {
        $sum = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        [IO.File]::WriteAllText("$file.sha256", "$sum  $([IO.Path]::GetFileName($file))`n")
    }
} finally { if (Test-Path $root) { Remove-Item -LiteralPath $root -Recurse -Force } }

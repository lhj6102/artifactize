# Test website/landing/install.ps1 against the zip built by binaries.yml. A local
# http server serves it in the GitHub release layout (v<version>/<archive>), and the
# script's internal ARTIFACTIZE_DOWNLOAD_URL points the installer at it.
#
#   .github/scripts/test-install.ps1 -Dist DIST [-Port 8737]
#
# DIST holds artifactize-v<version>-x86_64-pc-windows-msvc.zip and its .sha256.
# Installs into the default directory and the user Path, so run it on a CI runner:
# it uninstalls at the end. Runs in Windows PowerShell 5.1 and PowerShell 7.
param(
    [Parameter(Mandatory = $true)] [string] $Dist,
    [int] $Port = 8737
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$target = 'x86_64-pc-windows-msvc'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Fail([string] $message) { throw "FAIL: $message" }

# Pipe install.ps1 through iex as documented; return the error message, or $null.
function Install-Artifactize([string] $version, [string] $dir) {
    $env:ARTIFACTIZE_VERSION = $version
    if ($dir) { $env:ARTIFACTIZE_INSTALL_DIR = $dir } else { Remove-Item Env:ARTIFACTIZE_INSTALL_DIR -ErrorAction SilentlyContinue }
    try {
        Invoke-RestMethod -UseBasicParsing -Uri "$base/install.ps1" | Invoke-Expression
        return $null
    } catch {
        return "$_"
    }
}

function Get-UserPathEntries {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        $value = [string] $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    } finally { $key.Close() }
    return @($value -split ';' | Where-Object { $_ })
}

Write-Host "PowerShell $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition))"
$zip = @(Get-ChildItem -LiteralPath $Dist -Filter "artifactize-v*-$target.zip")
if ($zip.Count -ne 1) { Fail "expected one artifactize-v*-$target.zip in $Dist" }
$zip = $zip[0]
$version = $zip.Name -replace "^artifactize-v(.+)-$target\.zip$", '$1'
Write-Host "testing install.ps1 with artifactize $version"

$work = Join-Path ([IO.Path]::GetTempPath()) ('artifactize-test-' + [Guid]::NewGuid().ToString('N'))
$serve = Join-Path $work 'serve'
New-Item -ItemType Directory -Path (Join-Path $serve "v$version"), (Join-Path $serve 'v0.0.1') | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'website\landing\install.ps1') -Destination $serve
Copy-Item -LiteralPath $zip.FullName, "$($zip.FullName).sha256" -Destination (Join-Path $serve "v$version")
# Release 0.0.1 has a tampered zip: the real one plus a byte, under the real checksum.
$bad = Join-Path $serve "v0.0.1\artifactize-v0.0.1-$target.zip"
Copy-Item -LiteralPath $zip.FullName -Destination $bad
$stream = [IO.File]::Open($bad, [IO.FileMode]::Append)
try { $stream.WriteByte(120) } finally { $stream.Close() }
Copy-Item -LiteralPath "$($zip.FullName).sha256" -Destination "$bad.sha256"

$server = Start-Process -FilePath python -PassThru -WindowStyle Hidden `
    -ArgumentList @('-m', 'http.server', "$Port", '--bind', '127.0.0.1', '--directory', "`"$serve`"")
$base = "http://127.0.0.1:$Port"
$env:ARTIFACTIZE_DOWNLOAD_URL = $base
$defaultDir = Join-Path $env:LOCALAPPDATA 'Programs\artifactize'
$exe = Join-Path $defaultDir 'artifactize.exe'
$pathBefore = $env:Path
try {
    for ($i = 0; ; $i++) {
        try { Invoke-WebRequest -UseBasicParsing -Uri "$base/install.ps1" | Out-Null; break } catch {
            if ($i -ge 50) { throw }
            Start-Sleep -Milliseconds 200
        }
    }

    Write-Host '== fresh install into the default directory'
    if ((Test-Path -LiteralPath $defaultDir) -or ((Get-UserPathEntries) -contains $defaultDir)) {
        Fail "$defaultDir is installed already"
    }
    $err = Install-Artifactize $version $null
    if ($err) { Fail "install failed: $err" }
    $out = & $exe --version
    if ($out -ne "artifactize $version") { Fail "--version printed: $out" }
    if (@(Get-UserPathEntries | Where-Object { $_ -eq $defaultDir }).Count -ne 1) { Fail "$defaultDir is not on the user Path once" }
    $found = (Get-Command artifactize -CommandType Application | Select-Object -First 1).Path
    if ($found -ne $exe) { Fail "artifactize in this session resolves to $found" }
    & artifactize doctor
    if ($LASTEXITCODE -ne 0) { Fail 'artifactize doctor failed' }

    Write-Host '== update in place, with a v-prefixed version'
    $err = Install-Artifactize "v$version" $null
    if ($err) { Fail "update failed: $err" }
    if ((& $exe --version) -ne "artifactize $version") { Fail '--version after the update' }
    if (@(Get-UserPathEntries | Where-Object { $_ -eq $defaultDir }).Count -ne 1) { Fail 'the update added a second Path entry' }
    $files = @(Get-ChildItem -LiteralPath $defaultDir -Force | ForEach-Object { $_.Name })
    if (($files -join ',') -ne 'artifactize.exe') { Fail "unexpected files in ${defaultDir}: $($files -join ', ')" }

    Write-Host '== a checksum mismatch installs nothing'
    $mismatchDir = Join-Path $work 'mismatch'
    $err = Install-Artifactize '0.0.1' $mismatchDir
    if (-not $err) { Fail 'a tampered zip was installed' }
    Write-Host $err
    if ($err -notmatch 'checksum mismatch') { Fail "not a checksum error: $err" }
    if (Test-Path -LiteralPath (Join-Path $mismatchDir 'artifactize.exe')) { Fail 'the tampered binary was installed' }
    if ((Get-UserPathEntries) -contains $mismatchDir) { Fail 'a failed install changed the user Path' }

    Write-Host '== a missing release fails'
    $err = Install-Artifactize '9.9.9' (Join-Path $work 'missing')
    if (-not $err) { Fail 'a missing release installed' }
    Write-Host $err
    if ($err -notmatch 'cannot download') { Fail "not a download error: $err" }

    Write-Host '== uninstall as documented'
    Remove-Item -LiteralPath $defaultDir -Recurse -Force
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try {
        $kind = $key.GetValueKind('Path')
        $key.SetValue('Path', ((Get-UserPathEntries | Where-Object { $_ -ne $defaultDir }) -join ';'), $kind)
    } finally { $key.Close() }
    Write-Host 'all install.ps1 checks passed'
} finally {
    Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    $env:Path = $pathBefore
    Remove-Item Env:ARTIFACTIZE_DOWNLOAD_URL, Env:ARTIFACTIZE_VERSION, Env:ARTIFACTIZE_INSTALL_DIR -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

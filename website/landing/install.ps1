# Install artifactize from its GitHub release on Windows.
#
#   irm https://artifactize.dev/install.ps1 | iex
#
# Downloads the release zip for Windows x64 over https, checks its SHA-256 against
# the release's .sha256 file, installs artifactize.exe to
# %LOCALAPPDATA%\Programs\artifactize and adds that directory to the user Path.
# Needs no administrator rights. Works in Windows PowerShell 5.1 and PowerShell 7.
# Run it again to update.
#
# Environment:
#   ARTIFACTIZE_VERSION      install this release (0.5.2 or v0.5.2), not the latest
#   ARTIFACTIZE_INSTALL_DIR  install into this directory instead
#
# Internal, for this repository's CI only:
#   ARTIFACTIZE_DOWNLOAD_URL replaces https://github.com/lhj6102/artifactize/releases/download;
#                            archives are fetched from $env:ARTIFACTIZE_DOWNLOAD_URL/v<version>/.
#
# Everything runs in one script block, so its settings and variables stay out of
# the calling session, and an error stops the install without closing the window.

& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 downloads far more slowly while it draws a progress bar.
    $ProgressPreference = 'SilentlyContinue'

    $repo = 'lhj6102/artifactize'
    $target = 'x86_64-pc-windows-msvc'

    function Say([string] $message) {
        Write-Host "artifactize-install: $message"
    }

    function Fail([string] $message) {
        throw "artifactize-install: error: $message"
    }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        Fail 'needs Windows PowerShell 5.1 or PowerShell 7'
    }
    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
        Fail 'install.ps1 is for Windows; on Linux use https://artifactize.dev/install.sh'
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        Fail 'no prebuilt binary for 32-bit Windows'
    }
    # On ARM64, Windows 11 runs the x64 binary through emulation.

    # Windows PowerShell 5.1 may not offer TLS 1.2 by default; GitHub requires it.
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch { }

    $base = $env:ARTIFACTIZE_DOWNLOAD_URL
    if ($base) {
        Say "using the test download URL $base"
    } else {
        $base = "https://github.com/$repo/releases/download"
    }
    $base = $base.TrimEnd('/')

    $dir = $env:ARTIFACTIZE_INSTALL_DIR
    if (-not $dir) {
        if (-not $env:LOCALAPPDATA) { Fail 'LOCALAPPDATA is not set; set ARTIFACTIZE_INSTALL_DIR' }
        $dir = Join-Path $env:LOCALAPPDATA 'Programs\artifactize'
    }
    $dir = [IO.Path]::GetFullPath($dir).TrimEnd('\')

    $version = $env:ARTIFACTIZE_VERSION
    if (-not $version) {
        $api = "https://api.github.com/repos/$repo/releases/latest"
        try {
            $version = (Invoke-RestMethod -UseBasicParsing -Uri $api).tag_name
        } catch {
            Fail "cannot look up the latest release at ${api}: $($_.Exception.Message); set ARTIFACTIZE_VERSION to choose one"
        }
    }
    if ($version -like 'v*') { $version = $version.Substring(1) }
    if ($version -notmatch '^[0-9A-Za-z.+-]+$') { Fail "not a version: '$version'" }

    $name = "artifactize-v$version-$target"
    $zipName = "$name.zip"
    $url = "$base/v$version/$zipName"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('artifactize-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Say "downloading artifactize $version for $target"
        $zip = Join-Path $tmp $zipName
        foreach ($download in @(@($url, $zip), @("$url.sha256", "$zip.sha256"))) {
            try {
                Invoke-WebRequest -UseBasicParsing -Uri $download[0] -OutFile $download[1]
            } catch {
                Fail "cannot download $($download[0]): $($_.Exception.Message)"
            }
        }

        # The .sha256 file reads "<hex digest>  <archive name>".
        $expected = ([IO.File]::ReadAllText("$zip.sha256").Trim() -split '\s+')[0].ToLowerInvariant()
        if ($expected -notmatch '^[0-9a-f]{64}$') { Fail "malformed checksum file $url.sha256" }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            Fail "checksum mismatch for ${zipName}: expected $expected, got $actual; nothing was installed"
        }
        Say "checked SHA-256 $actual"

        Expand-Archive -LiteralPath $zip -DestinationPath $tmp
        $source = Join-Path $tmp "$name\artifactize.exe"
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { Fail "$zipName does not contain $name\artifactize.exe" }

        # Stage the binary next to its destination, then rename it into place. A running
        # artifactize.exe cannot be overwritten but can be renamed, so move it aside first.
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $exe = Join-Path $dir 'artifactize.exe'
        $staged = "$exe.new"
        $old = "$exe.old"
        Copy-Item -LiteralPath $source -Destination $staged -Force
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $exe) { Move-Item -LiteralPath $exe -Destination $old -Force }
        Move-Item -LiteralPath $staged -Destination $exe
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    $installed = & $exe --version
    if ($LASTEXITCODE -ne 0) { Fail "the installed $exe does not run" }
    Say "installed $installed to $exe"

    # Add the directory to the user Path in the registry. Reading and writing the raw
    # value keeps entries such as %USERPROFILE%\bin unexpanded; setx would truncate it.
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try {
        $userPath = [string] $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $entries = @($userPath -split ';' | Where-Object { $_ })
        $present = $entries | Where-Object { [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -eq $dir }
        if ($present) {
            Say "$dir is already on your user Path"
        } else {
            $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
            if ($key.GetValueNames() -contains 'Path') { $kind = $key.GetValueKind('Path') }
            $key.SetValue('Path', (($entries + $dir) -join ';'), $kind)
            # Tell running programs (Explorer, new terminals) that the environment changed:
            # setting a user variable through .NET broadcasts WM_SETTINGCHANGE.
            [Environment]::SetEnvironmentVariable('ARTIFACTIZE_INSTALL_BROADCAST', '1', 'User')
            [Environment]::SetEnvironmentVariable('ARTIFACTIZE_INSTALL_BROADCAST', $null, 'User')
            Say "added $dir to your user Path; new terminals will find artifactize"
        }
    } finally {
        $key.Close()
    }

    # This session too.
    if (-not (@($env:Path -split ';' | ForEach-Object { $_.TrimEnd('\') }) -contains $dir)) {
        $env:Path = "$env:Path;$dir"
    }
    $found = Get-Command artifactize -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found -and $found.Path -ne $exe) {
        Say "note: 'artifactize' on Path is $($found.Path), which comes before $dir"
    }
}

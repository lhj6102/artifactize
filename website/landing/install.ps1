# Install artifactize on Windows from its GitHub release.
#
#   irm https://artifactize.dev/install.ps1 | iex
#
# winget (lhj6102.Artifactize) and the Microsoft Store install the same commands; the
# Store version is signed, so prefer it where Smart App Control is on. See
# https://artifactize.dev/docs/getting-started/install.html
#
# Downloads the release zip for Windows x64 over https, checks its SHA-256 against
# the release's .sha256 file, installs artifactize.exe and artifactize-tools.exe to
# %LOCALAPPDATA%\Programs\artifactize and adds that directory to the user Path.
# Needs no administrator rights. Works in Windows PowerShell 5.1 and PowerShell 7.
# Run it again to update.
#
# Environment:
#   ARTIFACTIZE_VERSION      install this release (0.5.2 or v0.5.2) instead of the latest
#                            stable one; only this way installs a prerelease (0.6.0-alpha.1)
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
        Fail 'install.ps1 is for Windows; on Linux and macOS, use https://artifactize.dev/install.sh'
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
        $commands = @('artifactize', 'artifactize-tools')
        foreach ($command in $commands) {
            $source = Join-Path $tmp "$name\$command.exe"
            if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
                Fail "$zipName does not contain $name\$command.exe; nothing was installed"
            }
        }

        # Check both new commands before moving either installed binary or changing
        # Path. Application control may refuse an unsigned release even when the
        # previous release ran. Do not bypass the policy or discard a working install.
        foreach ($command in $commands) {
            $source = Join-Path $tmp "$name\$command.exe"
            try {
                $output = (& $source --version) -join ' '
                if ($LASTEXITCODE -ne 0) { throw "--version exited with code $LASTEXITCODE" }
                if ($output -ne "$command $version") { throw "--version printed '$output', not '$command $version'" }
            } catch {
                Fail "Windows could not run the new ${command}: $($_.Exception.Message). Existing binaries were kept and Path was not changed. Smart App Control or another application control policy may block this unsigned download. Install the signed Microsoft Store version instead: https://apps.microsoft.com/detail/9PB6W4LL165D"
            }
        }

        # Stage on the destination filesystem, then rename. A running .exe can be
        # renamed but not overwritten. Keep backups until both replacements succeed,
        # and restore the previous pair if any move fails.
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $stage = Join-Path $dir ('.artifactize-install-' + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $stage | Out-Null
        $backedUp = @()
        $replaced = @()
        $committed = $false
        try {
            foreach ($command in $commands) {
                Copy-Item -LiteralPath (Join-Path $tmp "$name\$command.exe") -Destination (Join-Path $stage "$command.exe")
            }
            foreach ($command in $commands) {
                $exe = Join-Path $dir "$command.exe"
                if (Test-Path -LiteralPath $exe) {
                    Move-Item -LiteralPath $exe -Destination (Join-Path $stage "$command.exe.old")
                    $backedUp += $command
                }
                Move-Item -LiteralPath (Join-Path $stage "$command.exe") -Destination $exe
                $replaced += $command
            }
            $committed = $true
        } catch {
            $installError = $_
            foreach ($command in $replaced) {
                Remove-Item -LiteralPath (Join-Path $dir "$command.exe") -Force
            }
            foreach ($command in $backedUp) {
                Move-Item -LiteralPath (Join-Path $stage "$command.exe.old") -Destination (Join-Path $dir "$command.exe")
            }
            $backedUp = @()
            throw $installError
        } finally {
            if ($committed -or $backedUp.Count -eq 0) {
                Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
            } else {
                Say "could not restore every previous binary; backups remain in $stage"
            }
        }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    foreach ($command in $commands) {
        Say "installed $command $version to $(Join-Path $dir "$command.exe")"
    }

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
            Say "added $dir to your user Path; new terminals will find artifactize and artifactize-tools"
        }
    } finally {
        $key.Close()
    }

    # This session too.
    if (-not (@($env:Path -split ';' | ForEach-Object { $_.TrimEnd('\') }) -contains $dir)) {
        $env:Path = "$env:Path;$dir"
    }
    # Every other copy on Path: one that comes first runs instead. A warning only.
    foreach ($command in $commands) {
        $exe = Join-Path $dir "$command.exe"
        $others = @(Get-Command $command -CommandType Application -All -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -ne $exe } | Select-Object -ExpandProperty Path -Unique)
        if ($others) {
            Say "another $command on your Path may run instead of this one:"
            foreach ($other in $others) {
                $otherVersion = try { (& $other --version 2>$null) -join ' ' } catch { 'version unknown' }
                Write-Host "    $other ($otherVersion)"
            }
            Say "remove that install (cargo: 'cargo uninstall $command'; winget: 'winget uninstall lhj6102.Artifactize') or put $dir first on your user Path, then open a new terminal."
        }
    }
}

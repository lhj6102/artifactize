# Run in Windows PowerShell 5.1 on the disposable packaging runner.
param(
    [Parameter(Mandatory = $true)][string] $Version,
    [Parameter(Mandatory = $true)][string] $Dist,
    [Parameter(Mandatory = $true)][string] $Installer
)
$ErrorActionPreference = 'Stop'
$Installer = (Resolve-Path $Installer).Path
$Dist = (Resolve-Path $Dist).Path
foreach ($script in @($Installer, $PSCommandPath)) {
    $tokens = $null
    $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($script, [ref] $tokens, [ref] $errors) | Out-Null
    if ($errors) { throw ($errors | Out-String) }
}

$root = Join-Path ([IO.Path]::GetTempPath()) ('artifactize-smoke-' + [Guid]::NewGuid().ToString('N'))
$serve = Join-Path $root 'serve'
$release = Join-Path $serve "v$Version"
New-Item -ItemType Directory -Path $release -Force | Out-Null
Copy-Item (Join-Path $Dist '*') $release
Copy-Item $Installer (Join-Path $serve 'install.ps1')
$server = Start-Process python -PassThru -WindowStyle Hidden `
    -ArgumentList '-m', 'http.server', '8737', '--bind', '127.0.0.1', '--directory', ('"' + $serve + '"')
try {
    $base = 'http://127.0.0.1:8737'
    for ($i = 0; ; $i++) {
        try { Invoke-WebRequest -UseBasicParsing "$base/install.ps1" | Out-Null; break }
        catch { if ($i -ge 50) { throw }; Start-Sleep -Milliseconds 200 }
    }
    $env:ARTIFACTIZE_DOWNLOAD_URL = $base
    $env:ARTIFACTIZE_VERSION = $Version
    $env:ARTIFACTIZE_INSTALL_DIR = Join-Path $root 'installed'
    irm "$base/install.ps1" | iex
    $commands = @('artifactize', 'artifactize-tools')
    foreach ($command in $commands) {
        $out = & $command --version
        if ($LASTEXITCODE -ne 0 -or $out -ne "$command $Version") { throw "$command --version printed: $out" }
    }
    & artifactize --state-dir (Join-Path $root 'doctor-state') doctor
    if ($LASTEXITCODE -ne 0) { throw 'artifactize doctor failed' }
    # The script's receipt names it as the way to update; the lookup itself stays offline.
    $env:ARTIFACTIZE_NO_UPDATE_CHECK = '1'
    $report = & artifactize --state-dir (Join-Path $root 'doctor-state') doctor --json | Out-String | ConvertFrom-Json
    Remove-Item Env:ARTIFACTIZE_NO_UPDATE_CHECK
    $channel = ($report.checks | Where-Object name -eq 'release').details.channel
    if ($channel -ne 'installPs1') { throw "artifactize doctor names the channel '$channel', not installPs1" }

    # The desktop app beside the CLI that starts its Runs, opened from its Start shortcut.
    $desktop = Join-Path $env:ARTIFACTIZE_INSTALL_DIR 'artifactize-desktop.exe'
    if (-not (Test-Path -LiteralPath $desktop)) { throw 'install.ps1 did not install artifactize-desktop.exe' }
    $link = Join-Path ([Environment]::GetFolderPath('Programs')) 'Artifactize.lnk'
    if ((New-Object -ComObject WScript.Shell).CreateShortcut($link).TargetPath -ne $desktop) { throw "$link does not open $desktop" }
    Start-Process $link
    for ($i = 0; -not (Get-Process artifactize-desktop -ErrorAction SilentlyContinue | Where-Object MainWindowTitle -eq 'Artifactize'); $i++) {
        if ($i -ge 120) { throw 'the installed desktop app showed no Artifactize window' }; Start-Sleep -Milliseconds 500
    }
    Get-Process artifactize-desktop | Stop-Process -Force
    Remove-Item -LiteralPath $link
    Write-Host 'install.ps1 desktop app opened its window from the Start menu'

    $shadow = Join-Path $root 'shadow'
    New-Item -ItemType Directory -Path $shadow | Out-Null
    Copy-Item (Join-Path $env:ARTIFACTIZE_INSTALL_DIR '*.exe') $shadow
    $env:Path = "$shadow;$env:Path"
    $log = Join-Path $root 'shadow.log'
    & $Installer *> $log
    Get-Content $log
    foreach ($command in $commands) {
        if (-not (Select-String -LiteralPath $log -SimpleMatch "another $command on your Path")) {
            throw "missing shadowing warning for $command"
        }
    }

    # Make the previous pair byte-distinct from the offered update while keeping it
    # runnable: Windows ignores an overlay after the PE image. This detects replacing
    # the healthy command before discovering that the other new command is blocked.
    foreach ($command in $commands) {
        $exe = Join-Path $env:ARTIFACTIZE_INSTALL_DIR "$command.exe"
        $stream = [IO.File]::Open($exe, [IO.FileMode]::Append, [IO.FileAccess]::Write)
        try {
            $marker = [Text.Encoding]::UTF8.GetBytes("previous-install-$command")
            $stream.Write($marker, 0, $marker.Length)
        } finally { $stream.Dispose() }
        $out = & $exe --version
        if ($LASTEXITCODE -ne 0 -or $out -ne "$command $Version") { throw "marked previous $command does not run" }
    }

    # A checksum-valid archive whose .exe cannot start simulates application control.
    # Corrupt each command in turn: even when artifactize runs, a blocked tools binary
    # must leave BOTH installed files and both process/user Path values untouched.
    $name = "artifactize-v$Version-x86_64-pc-windows-msvc"
    $zip = Join-Path $release "$name.zip"
    foreach ($blocked in $commands) {
        $fixture = Join-Path $root "blocked-$blocked"
        Expand-Archive -LiteralPath (Join-Path $Dist "$name.zip") -DestinationPath $fixture
        [IO.File]::WriteAllText((Join-Path $fixture "$name\$blocked.exe"), 'not a runnable Windows executable')
        Remove-Item -LiteralPath $zip
        Compress-Archive -LiteralPath (Join-Path $fixture $name) -DestinationPath $zip
        $sum = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        [IO.File]::WriteAllText("$zip.sha256", "$sum  $name.zip`n")
        $before = @{}
        foreach ($command in $commands) {
            $before[$command] = (Get-FileHash -LiteralPath (Join-Path $env:ARTIFACTIZE_INSTALL_DIR "$command.exe")).Hash
        }
        $processPath = $env:Path
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $message = $null
        try { & $Installer } catch { $message = $_.Exception.Message }
        if (-not $message) { throw "installer accepted the non-runnable $blocked" }
        Write-Host $message
        foreach ($expected in @("new ${blocked}", 'Existing binaries were kept', 'application control policy', 'Do not bypass', 'Microsoft Store', '9PB6W4LL165D', 'https://artifactize.dev/docs/getting-started/install.html')) {
            if (-not $message.Contains($expected)) { throw "missing blocked-binary explanation: $expected" }
        }
        foreach ($command in $commands) {
            $exe = Join-Path $env:ARTIFACTIZE_INSTALL_DIR "$command.exe"
            if ((Get-FileHash -LiteralPath $exe).Hash -ne $before[$command]) { throw "replaced $command after $blocked failed to start" }
            $out = & $exe --version
            if ($LASTEXITCODE -ne 0 -or $out -ne "$command $Version") { throw "previous $command no longer runs" }
        }
        if ($env:Path -ne $processPath -or [Environment]::GetEnvironmentVariable('Path', 'User') -ne $userPath) {
            throw 'changed Path after the staged binary failed to start'
        }
    }
} finally {
    Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

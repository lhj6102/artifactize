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
        foreach ($expected in @("new ${blocked}", 'Existing binaries were kept', 'application control policy', 'Microsoft Store', 'https://apps.microsoft.com/detail/9PB6W4LL165D')) {
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

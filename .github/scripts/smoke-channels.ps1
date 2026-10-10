# Disposable Windows runner only. Never submit to a distribution service.
param(
    [Parameter(Mandatory = $true)][string] $Version,
    [Parameter(Mandatory = $true)][string] $Dist
)
$ErrorActionPreference = 'Stop'
$Dist = (Resolve-Path $Dist).Path
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
foreach ($script in Get-ChildItem (Join-Path $PSScriptRoot '*.ps1')) {
    $tokens = $null; $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref] $tokens, [ref] $errors) | Out-Null
    if ($errors) { throw ($errors | Out-String) }
}
$root = Join-Path $env:TEMP ('artifactize-channels-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$server = $null; $cert = $null
$packageName = 'lhj6102.Artifactize'
try {
    # Hosted server provides exactly the PR-built zip; no public release is read.
    $server = Start-Process python -PassThru -WindowStyle Hidden -ArgumentList `
        '-I', '-m', 'http.server', '8738', '--bind', '127.0.0.1', '--directory', ('"' + $Dist + '"')
    for ($i = 0; ; $i++) {
        try { Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:8738/' | Out-Null; break }
        catch { if ($i -ge 50) { throw }; Start-Sleep -Milliseconds 200 }
    }
    $manifests = Join-Path $root 'winget'
    & python -I (Join-Path $PSScriptRoot 'channels.py') generate winget --version $Version --dist $Dist `
        --output $manifests --base-url 'http://127.0.0.1:8738'
    if ($LASTEXITCODE -ne 0) { throw 'winget generation failed' }
    # windows-latest is Windows Server: App Installer/winget is not guaranteed.
    # Microsoft's maintained module bootstraps its supported client on that image.
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        Install-PackageProvider -Name NuGet -Force -Scope CurrentUser | Out-Null
        Install-Module Microsoft.WinGet.Client -Force -Scope CurrentUser -Repository PSGallery
        Repair-WinGetPackageManager -AllUsers
    }
    & winget --info
    & winget validate --manifest $manifests --disable-interactivity
    if ($LASTEXITCODE -ne 0) { throw 'winget validate failed' }
    & winget settings --enable LocalManifestFiles
    if ($LASTEXITCODE -ne 0) { throw 'enabling local winget manifests failed' }
    & winget install --manifest $manifests --scope user --accept-package-agreements --accept-source-agreements --disable-interactivity
    if ($LASTEXITCODE -ne 0) { throw 'winget local install failed' }
    # Refresh PATH as a new terminal would; do not run files directly from the zip.
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
    foreach ($command in @('artifactize', 'artifactize-tools')) {
        $out = & $command --version
        if ($LASTEXITCODE -ne 0 -or $out -ne "$command $Version") { throw "winget alias $command failed: $out" }
    }
    & winget uninstall --id lhj6102.Artifactize --exact --disable-interactivity
    if ($LASTEXITCODE -ne 0) { throw 'winget uninstall failed' }

    & (Join-Path $PSScriptRoot 'build-msix.ps1') -Version $Version -Dist $Dist -Output $root
    $package = Join-Path $root "artifactize-v$Version-x64.msix"
    $cert = New-SelfSignedCertificate -Type Custom -Subject 'CN=F922229C-8605-4186-87BA-B5FF18595D66' `
        -KeyUsage DigitalSignature -FriendlyName 'Artifactize disposable CI' -CertStoreLocation 'Cert:\CurrentUser\My' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
    $password = ConvertTo-SecureString 'disposable-ci-only' -AsPlainText -Force
    $pfx = Join-Path $root 'test.pfx'; $cer = Join-Path $root 'test.cer'
    Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $password | Out-Null
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
    $sign = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
        Sort-Object FullName -Descending | Select-Object -First 1
    & $sign.FullName sign /fd SHA256 /f $pfx /p 'disposable-ci-only' $package
    if ($LASTEXITCODE -ne 0) { throw 'MSIX test signing failed' }
    Add-AppxPackage -Path $package
    $installed = Get-AppxPackage -Name $packageName
    if ($installed.PackageFamilyName -ne 'lhj6102.Artifactize_ayfzfzbsv48sg') { throw 'Wrong package family' }
    $aliases = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps'
    foreach ($command in @('artifactize', 'artifactize-tools')) {
        $alias = Join-Path $aliases "$command.exe"
        for ($i = 0; -not (Test-Path $alias); $i++) {
            if ($i -ge 50) { throw "MSIX alias $command missing" }; Start-Sleep -Milliseconds 200
        }
        $out = & $alias --version
        if ($LASTEXITCODE -ne 0 -or $out -ne "$command $Version") { throw "MSIX alias $command failed: $out" }
    }
    # The un-packaged shell inspects the real directory after a packaged alias writes.
    # Clear overrides, not LOCALAPPDATA: test the actual Windows known folder.
    Remove-Item Env:ARTIFACTIZE_STATE_HOME, Env:XDG_STATE_HOME -ErrorAction SilentlyContinue
    $state = Join-Path $env:LOCALAPPDATA 'artifactize'
    if (Test-Path $state) { throw 'State seam requires a fresh disposable runner' }
    & (Join-Path $aliases 'artifactize.exe') server token list
    if ($LASTEXITCODE -ne 0) { throw 'Packaged artifactize state write failed' }
    if (-not (Get-ChildItem $state -Recurse -File | Where-Object { $_.Extension -eq '.sqlite' })) {
        throw 'MSIX did not write SQLite state to real LOCALAPPDATA\artifactize'
    }
    $private = Join-Path $env:LOCALAPPDATA "Packages\$($installed.PackageFamilyName)"
    $redirected = Get-ChildItem $private -Recurse -Filter '*.sqlite' -ErrorAction SilentlyContinue
    if ($redirected) { throw "State was virtualized: $($redirected.FullName)" }
    Write-Host 'MSIX aliases and unvirtualized state passed'
} finally {
    Get-AppxPackage -Name $packageName | Remove-AppxPackage
    if ($cert) {
        Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
        Remove-Item "Cert:\LocalMachine\TrustedPeople\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    }
    if ($server) { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $root -Recurse -Force
}

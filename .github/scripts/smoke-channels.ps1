# Disposable Windows runner only. Never submit to a distribution service.
param(
    [Parameter(Mandatory = $true)][string] $Version,
    [Parameter(Mandatory = $true)][string] $Dist,
    [ValidateSet('winget', 'msix')][string] $Channel = 'msix',
    [switch] $DesktopE2E
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
$server = $null; $cert = $null; $childName = $null
$packageName = 'lhj6102.Artifactize'
$removePackage = -not $DesktopE2E
try {
    if ($Channel -eq 'winget') {
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
        & winget uninstall --manifest $manifests --scope user --accept-source-agreements --disable-interactivity
        if ($LASTEXITCODE -ne 0) { throw 'winget uninstall failed' }
    } else {
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
        $alias = Join-Path $aliases 'artifactize.exe'
        for ($i = 0; -not (Test-Path $alias); $i++) {
            if ($i -ge 50) { throw 'MSIX artifactize alias missing' }; Start-Sleep -Milliseconds 200
        }
        $out = & $alias --version
        if ($LASTEXITCODE -ne 0 -or $out -ne "artifactize $Version") { throw "MSIX alias failed: $out" }
        $manifest = Get-AppxPackageManifest -Package $installed
        $applications = @($manifest.Package.Applications.Application)
        if ($applications.Count -ne 2 -or @($applications.Id) -notcontains 'Artifactize' -or @($applications.Id) -notcontains 'Desktop') {
            throw 'MSIX must expose the existing CLI entry and the desktop Start menu entry'
        }
        $tools = Join-Path $installed.InstallLocation 'artifactize-tools.exe'
        $out = & $tools --version
        if ($LASTEXITCODE -ne 0 -or $out -ne "artifactize-tools $Version") { throw "MSIX bundled tools failed: $out" }
        if ($DesktopE2E) {
            & python -I (Join-Path $PSScriptRoot 'desktop.py') installed --target x86_64-pc-windows-msvc `
                --version $Version --app (Join-Path $installed.InstallLocation 'artifactize-desktop.exe')
            if ($LASTEXITCODE -ne 0) { throw 'Installed MSIX desktop journeys failed; retain the package until runner teardown' }
            $removePackage = $true
        } else {
            # Resolve and activate the installed Start menu entry, not a loose build.
            $aumid = "$($installed.PackageFamilyName)!Desktop"
            for ($i = 0; ; $i++) {
                $entry = @(Get-StartApps | Where-Object AppID -eq $aumid)
                if ($entry.Count -eq 1) { break }
                if ($i -ge 50) { throw "Desktop Start menu entry missing: $aumid" }
                Start-Sleep -Milliseconds 200
            }
            Start-Process explorer.exe -ArgumentList "shell:AppsFolder\$aumid"
            $desktop = $null
            try {
                for ($i = 0; $i -lt 100; $i++) {
                    $desktop = Get-Process -Name artifactize-desktop -ErrorAction SilentlyContinue |
                        Where-Object { $_.Path -eq (Join-Path $installed.InstallLocation 'artifactize-desktop.exe') -and $_.MainWindowHandle -ne 0 } |
                        Select-Object -First 1
                    if ($desktop) { break }
                    Start-Sleep -Milliseconds 200
                }
                if (-not $desktop) { throw 'Start menu desktop did not open a window' }
            } finally {
                Get-Process -Name artifactize-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
            }
        }
        $sample = Join-Path $root 'sample.txt'
        [IO.File]::WriteAllText($sample, 'artifactize MSIX tools seam')
        Push-Location $root
        try {
            $out = & $alias tools read sample.txt
            if ($LASTEXITCODE -ne 0 -or $out -ne 'artifactize MSIX tools seam') { throw "MSIX tools facade failed: $out" }
            & $alias tools read ../escape
            if ($LASTEXITCODE -ne 1) { throw 'MSIX tools facade must refuse scope escapes' }
        } finally { Pop-Location }
        # Inspect physical files from this unpackaged shell, not through the alias.
        # Clear overrides, not LOCALAPPDATA: test the actual Windows known folder.
        Remove-Item Env:ARTIFACTIZE_STATE_HOME, Env:XDG_STATE_HOME -ErrorAction SilentlyContinue
        $state = Join-Path $env:LOCALAPPDATA 'artifactize'
        if (Test-Path $state) { throw 'State seam requires a fresh disposable runner' }
        & $alias server token list
        if ($LASTEXITCODE -ne 0) { throw 'Packaged artifactize state write failed' }
        $private = Join-Path $env:LOCALAPPDATA "Packages\$($installed.PackageFamilyName)"
        $databases = @(Get-ChildItem $private -Recurse -Filter 'review-store.sqlite' -ErrorAction SilentlyContinue)
        if ($databases.Count -ne 1) { throw "Expected one package-private review-store.sqlite, found $($databases.Count)" }
        if (Test-Path $state) { throw 'MSIX wrote state to real LOCALAPPDATA\artifactize' }
        Write-Host "MSIX review-store database: $($databases[0].FullName)"
        Write-Host "MSIX virtualization runner: $([Environment]::OSVersion.VersionString)"

        # A real Runtime eval exercises artifactize's worker/child-process launch path.
        # Use a unique new file: existing AppData files have different overlay semantics.
        $probe = Join-Path $root 'virtualization-probe'
        New-Item -ItemType Directory -Path $probe | Out-Null
        $childName = 'artifactize-msix-child-' + [Guid]::NewGuid().ToString('N') + '.txt'
        $writer = '[IO.File]::WriteAllText((Join-Path $env:LOCALAPPDATA ''@NAME@''), ''MSIX runtime child'')'
        [IO.File]::WriteAllText((Join-Path $probe 'write-appdata.ps1'), $writer.Replace('@NAME@', $childName))
        [IO.File]::WriteAllText((Join-Path $probe 'index.artf'), @'
name = "virtualization-probe"
[evals.appdata]
title = "Measure packaged child AppData writes"
profile = { kind = "runtime", command = "powershell.exe", args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "write-appdata.ps1"] }
[evals.appdata.payload]
instruction = "Write a new AppData file to measure packaged child-process redirection."
'@)
        & $alias verify --repo $probe --all --wait
        if ($LASTEXITCODE -ne 0) { throw 'MSIX child-process Runtime eval failed' }
        $databases = @(Get-ChildItem $private -Recurse -Filter 'state.sqlite' -ErrorAction SilentlyContinue)
        if ($databases.Count -ne 1) { throw "Expected one package-private state.sqlite, found $($databases.Count)" }
        if (Test-Path $state) { throw 'MSIX worker wrote state to real LOCALAPPDATA\artifactize' }
        Write-Host "MSIX state database: $($databases[0].FullName)"
        $realChild = Join-Path $env:LOCALAPPDATA $childName
        $privateChildren = @(Get-ChildItem $private -Recurse -Filter $childName -ErrorAction SilentlyContinue)
        if ($privateChildren.Count -eq 1 -and -not (Test-Path $realChild)) {
            Write-Host "MSIX child AppData redirected: yes; file: $($privateChildren[0].FullName)"
        } elseif ($privateChildren.Count -eq 0 -and (Test-Path $realChild)) {
            Write-Host "MSIX child AppData redirected: no; file: $realChild"
        } else { throw 'MSIX child AppData probe has missing or ambiguous physical files' }
        Write-Host 'MSIX alias, tools facade and package-private state passed'
    }
} finally {
    if ($Channel -eq 'msix' -and $removePackage) { Get-AppxPackage -Name $packageName | Remove-AppxPackage }
    if ($childName) { Remove-Item -LiteralPath (Join-Path $env:LOCALAPPDATA $childName) -Force -ErrorAction SilentlyContinue }
    if ($cert) {
        Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
        Remove-Item "Cert:\LocalMachine\TrustedPeople\$($cert.Thumbprint)" -ErrorAction SilentlyContinue
    }
    if ($server) { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $root -Recurse -Force
}

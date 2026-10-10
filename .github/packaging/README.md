# Release channels

The Linux tests and builds still gate the GitHub release. Other platforms and
channels are independent, best-effort jobs. All archives/installers contain both
`artifactize` and `artifactize-tools`. Missing channel credentials produce an
explicit skip message; they never fail a release. These scripts never sign or
submit a Windows zip.

## Policy and credentials

Create these secrets in **lhj6102/artifactize → Settings → Environments → release
→ Environment secrets** (repository Actions secrets also work). Do not put their
values in scripts, command history or issue comments.

| Channel | Policy | Secrets |
| --- | --- | --- |
| Homebrew | Stable only; a normal tap formula tracks stable, not prereleases | `HOMEBREW_TAP_TOKEN` |
| winget | Stable only | `WINGET_TOKEN` |
| Microsoft Store | Stable only; any pending submission is left untouched and logged/skipped | `STORE_TENANT_ID`, `STORE_CLIENT_ID`, `STORE_CLIENT_SECRET` |

Homebrew deliberately does not promote prereleases into the ordinary formula.
Prereleases still carry the winget metadata and unsigned MSIX/Store upload for
manual rehearsal. Neither metadata generation nor installation smoke tests use
channel credentials.

### Homebrew first setup

1. Owner creates **lhj6102/homebrew-tap**, initializes its default branch (e.g.
   with a README), and allows direct formula commits by the release identity.
2. At GitHub **Settings → Developer settings → Personal access tokens →
   Fine-grained tokens**, create a token restricted to that repository, with
   **Contents: Read and write**. Save it as `HOMEBREW_TAP_TOKEN` above.
3. The next stable release commits `Formula/artifactize.rb`. Install using
   `brew install lhj6102/tap/artifactize`.

The generator computes hashes from the two actual macOS release archives, not
from checksums supplied by an external endpoint:

```sh
python3 -I .github/scripts/channels.py generate homebrew \
  --version 1.2.3 --dist dist --output formula
```

### winget first submission

Identifier: **lhj6102.Artifactize**. Each release attaches
`winget-manifests.zip` with the version, defaultLocale and installer manifests.
The installer is a zip with two nested portable files and command aliases.

1. Owner downloads/unpacks the manifests from a **stable** GitHub release.
2. Run `winget validate --manifest <directory>` and install locally with
   `winget install --manifest <directory>` (enable `LocalManifestFiles` as admin).
3. Submit the first set manually with Microsoft's
   [wingetcreate](https://github.com/microsoft/winget-create):
   `wingetcreate submit --no-open <directory>`. Wait for its winget-pkgs PR to merge.
4. Create a GitHub **classic PAT** in Developer settings with **public_repo**
   (Microsoft's wingetcreate authentication requirement), under an account able to
   fork/open PRs to **microsoft/winget-pkgs**. Store it as `WINGET_TOKEN` above.
   Only then enable automatic later submissions.

Automation uses `wingetcreate submit` on freshly generated manifests instead of
`update`: the nested archive directory includes the version and must change too.
Microsoft's documented standalone `https://aka.ms/wingetcreate/latest` executable
is downloaded only in an enabled stable release job. Tokens are passed to the
creator process, never included in generated metadata or logged by our scripts.

### Microsoft Store first submission

Product: **Artifactize**, Store ID **9PB6W4LL165D**.

- MSIX Name: `lhj6102.Artifactize`
- Publisher: `CN=F922229C-8605-4186-87BA-B5FF18595D66`
- PublisherDisplayName: `lhj6102`
- Package family: `lhj6102.Artifactize_ayfzfzbsv48sg`
- Min OS: Windows 10 1903 (18362); architecture x64
- Version: `major.minor.patch.0` (Store revision must be zero; components ≤65535)

On a Windows SDK host, reproduce the unsigned release upload with:

```powershell
.github/scripts/build-msix.ps1 -Version 1.2.3 -Dist dist -Output channel-dist/store
```

Every Windows build retains the unsigned `.msix`, `.msixupload`, checksums and
winget manifests in Actions artifact **channels-windows**. The release attaches
the same files. Do not upload the throwaway CI-signed package; it is test-only.

1. In **Partner Center → Apps and games → Artifactize**, complete the first
   submission manually: listing, screenshots, age ratings, pricing/availability,
   and packages. Upload the release `.msixupload`. Explain the restricted
   `runFullTrust` and `unvirtualizedResources` capabilities. The latter is needed
   so Store and zip/winget installs share runs, authentication and cache under
   real `%LOCALAPPDATA%\artifactize`, and retain this state after uninstall.
   First submission must be published before automation can clone it.
2. In **Partner Center → Account settings → Tenants**, associate a Microsoft
   Entra tenant. In **Account settings → User management**, add an Entra
   application with the **Manager** role (an app registered in **Entra admin
   center → App registrations** can be linked here).
3. Obtain its tenant ID, application/client ID, and a client secret/key from the
   linked application settings (or **Entra → App registrations → Certificates
   & secrets**). Save them as `STORE_TENANT_ID`, `STORE_CLIENT_ID`, and
   `STORE_CLIENT_SECRET` respectively. Rotate the secret before expiration.
   `STORE_SELLER_ID` is **not required**: the documented MSIX submission API
   selects the account from the linked Entra app and the fixed Store product ID.
4. Later stable releases call Microsoft's official submission API, clone the
   published listing/ratings/pricing, mark old packages for deletion, upload the
   new package, then commit. Certification is asynchronous. Any pending
   submission (including certification, drafts or failures) is skipped without
   deleting it. Finish/delete a blocked draft manually before the next release.
   If a channel was skipped, use the retained release upload for manual submission;
   a subsequent stable release also retries by creating its own new submission.

No Store credentials or Partner Center changes are needed for build/test.
The package declares `desktop6:FileSystemWriteVirtualization` as `disabled` plus
`rescap:Capability Name="unvirtualizedResources"`; it does not change registry
virtualization. Required PNG logos are resized from the existing brand raster.
S8 may replace these placeholder package visuals.

Microsoft references used for the package and API:

- [Flexible virtualization](https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization)
- [AppExecutionAlias console subsystem](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap5-appexecutionalias)
- [Submission API prerequisites/authentication](https://learn.microsoft.com/en-us/windows/uwp/monetize/create-and-manage-submissions-using-windows-store-services)
- [App submissions and package fields](https://learn.microsoft.com/en-us/windows/uwp/monetize/manage-app-submissions)
- [Official Python submission example](https://learn.microsoft.com/en-us/windows/uwp/monetize/python-code-examples-for-the-windows-store-submission-api)

## Packaging seams

`binaries.yml` runs on packaging PRs and manual rehearsals. Its Linux policy job
runs `python3 -I .github/scripts/test-channels.py`: generated metadata, prerelease
skips, missing credentials, simulated certification/drafts, and the Store
clone/update/upload/commit sequence are checked without network submission.

The two informational macOS builds install from a native PR archive through a
local tap, run `brew test`, and execute both commands. The informational Windows
build parses all PowerShell scripts, validates/installs local winget manifests,
executes both portable aliases, and uninstalls them before the MSIX seam. Hosted
Windows Server images may omit winget: the test uses Microsoft's maintained
`Microsoft.WinGet.Client` / `Repair-WinGetPackageManager` bootstrap rather than
silently substituting metadata checks for installation.

The MSIX seam packs with SDK schema validation, signs a *copy* with a throwaway
certificate, installs via `Add-AppxPackage`, invokes both WindowsApps execution
aliases, then runs offline `artifactize server token list` (no verify/provider
calls). The unpackaged shell checks that SQLite state was created in real
`%LOCALAPPDATA%\artifactize`, not anywhere in the package-private directory. The
test removes its package and certificate. Windows/macOS remain non-gating before
v1, so inspect individual step outcomes, not just the overall workflow conclusion.

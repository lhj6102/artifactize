# Release channels

The Linux tests and builds still gate the GitHub release. Other platforms and
channels are independent, best-effort jobs. Archives and non-Store installers contain both
`artifactize` and `artifactize-tools`; the Store MSIX contains only `artifactize`,
with tools exposed through `artifactize tools`. The desktop app ships beside those
commands, because it starts Runs through the `artifactize` next to its own executable:
in the Windows zip and the MSIX (its own Start entry beside the CLI's),
and as the macOS dmg and the Linux AppImage and deb (`desktop-<target>` artifacts). Missing channel credentials produce an
explicit skip message; they never fail a release. Every expected skip also emits an
Actions warning annotation and a job-summary line, so a green skipped job cannot
be mistaken for a publication. Successful publications write their outcome to the
summary too (Store submission ID, winget PR URL when available, or Homebrew formula). These scripts never sign or
submit a Windows zip.

winget and Microsoft Store distribution are in preparation, not available yet:
the winget submissions are still open and the Store submission has not passed
certification. Use the PowerShell install script for Windows today; the setup and
publication instructions below describe the prepared channels.

## Policy and credentials

Create these secrets in **lhj6102/artifactize → Settings → Environments → release
→ Environment secrets** (repository Actions secrets also work). Do not put their
values in scripts, command history or issue comments.

| Channel | Policy | Secrets |
| --- | --- | --- |
| Homebrew | Stable only; a normal tap formula tracks stable, not prereleases | `HOMEBREW_TAP_TOKEN` |
| winget | Stable only | `WINGET_TOKEN` |
| Microsoft Store | Stable only; any pending submission is left untouched and logged/skipped | `STORE_TENANT_ID`, `STORE_CLIENT_ID`, `STORE_CLIENT_SECRET` |
| macOS desktop signing | Every build, PRs included; without them the app is signed ad hoc and only runs where it was built | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` |

Homebrew deliberately does not promote prereleases into the ordinary formula.
Prereleases still carry the winget metadata and unsigned MSIX/Store upload for
manual rehearsal. Neither metadata generation nor installation smoke tests use
channel credentials.

The macOS signing secrets must be repository Actions secrets: PR and rehearsal
builds read them too, not only the release environment. They are Tauri's
[macOS signing](https://v2.tauri.app/distribute/sign/macos/) variables: the base64
Developer ID Application certificate (.p12) and its password, the identity name, and
an Apple ID with an app-specific password and team ID for notarization.

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
   `runFullTrust` capability only: Artifactize is a desktop CLI that reads the
   user's selected projects and launches their configured evaluation/tool commands
   with the user's rights. It does not request `unvirtualizedResources`; new AppData
   state uses Windows' default package-private virtualization and is removed on uninstall.
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
The package uses Microsoft's default file-system and registry virtualization. It
has no virtualization-disable properties, no Flexible Virtualization exclusions and
no `unvirtualizedResources` capability; `runFullTrust` remains. Windows 10 1903
support is unchanged. New AppData state is package-private, not shared Runs,
authentication or cache with zip/winget/Cargo installs, and Windows removes that
private state on uninstall. This is not a sandbox: on supported Windows versions,
Microsoft documents that reads can fall back to existing unvirtualized AppData files
and writes to those existing files can affect the real files. Explicit state outside
AppData and user project/exported files are not removed with the package.
The MSIX has two visible Applications: the desktop app (`ArtifactizeDesktop`,
listed as Artifactize) and the CLI facade (`Artifactize`, listed as Artifactize CLI)
with one console execution alias (`artifactize.exe`); tools run as
`artifactize tools <subcommand>`. The desktop app starts Runs through the
`artifactize.exe` beside it. Neither is headless or has `AppListEntry="none"`, so no
HeadlessAppBypass waiver is needed. MakeAppx accepts only one appExecutionAlias extension per
Application; the standalone tools executable remains in all non-Store channels.
Launching Artifactize from Start or Explorer with no arguments shows the normal
help, a terminal hint and waits for Enter only when stdin/stdout are consoles and
this process is the console's only attached process. Terminal launches, arguments
and redirection never add a pause.
Required PNG logos are resized from the existing brand raster.
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
executes both portable aliases, and uninstalls them. The MSIX seam runs first in a
separate step so a winget bootstrap limitation cannot hide its outcome. Hosted
Windows Server images may omit winget: the test uses Microsoft's maintained
`Microsoft.WinGet.Client` / `Repair-WinGetPackageManager` bootstrap rather than
silently substituting metadata checks for installation.

The MSIX seam takes the retained `.msix` (packed with SDK schema validation), checks that
the retained `.msixupload` carries exactly that package, signs a *copy* with a throwaway
certificate, installs via `Add-AppxPackage`, invokes the WindowsApps `artifactize` execution
alias and its tools facade, then runs offline `artifactize server token list` and a
Runtime-only verification that creates a unique new AppData file in a child PowerShell
process (no provider calls). The unpackaged shell checks that both SQLite databases
are package-private and that real `%LOCALAPPDATA%\artifactize` was not created. Logs
record the actual database paths, runner OS and whether the child's new AppData file
was redirected. Last, it starts the desktop app from its Start entry and waits for its
window. This checks a fresh runner, not migration from existing unpackaged state. The test removes its package, certificate and probe files.

[Windows rehearsal 39138230810](https://github.com/lhj6102/artifactize/actions/runs/39138230810)
passed on Windows Server build 26100 (`windows-2025-vs2026`). Both `state.sqlite` and
`review-store.sqlite` landed under
`%LOCALAPPDATA%\Packages\lhj6102.Artifactize_ayfzfzbsv48sg\LocalCache\Local\artifactize`,
and real `%LOCALAPPDATA%\artifactize` was absent. The Runtime PowerShell child's new
AppData file was **not redirected**: it landed in real `%LOCALAPPDATA%`, not beneath
the package. Do not extend this one runner/command observation to all child programs
or supported Windows versions; command-created files outside the private view can
remain after uninstall.

Windows/macOS remain non-gating before v1, so inspect individual step outcomes, not
just the overall workflow conclusion.

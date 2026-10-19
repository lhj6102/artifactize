//! A scoped denial of new files, restored before session teardown on every OS.
use std::path::Path;

#[cfg(unix)]
pub type CreationDenied = super::ReadOnly;

#[cfg(unix)]
pub fn deny_creates(directory: &Path) -> CreationDenied {
    super::read_only(directory)
}

#[cfg(windows)]
pub struct CreationDenied {
    directory: std::path::PathBuf,
    inputs: tempfile::TempDir,
}

#[cfg(windows)]
pub fn deny_creates(directory: &Path) -> CreationDenied {
    let inputs = super::tempdir();
    std::fs::write(inputs.path().join("deny.ps1"), r#"param([string]$Target, [string]$Saved)
$ErrorActionPreference = 'Stop'
$acl = Get-Acl -LiteralPath $Target
$sections = [System.Security.AccessControl.AccessControlSections]::Access
[System.IO.File]::WriteAllText($Saved, $acl.GetSecurityDescriptorSddlForm($sections))
$everyone = [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0')
$deny = [System.Security.AccessControl.FileSystemAccessRule]::new($everyone, [System.Security.AccessControl.FileSystemRights]::CreateFiles, [System.Security.AccessControl.AccessControlType]::Deny)
$acl.AddAccessRule($deny)
Set-Acl -LiteralPath $Target -AclObject $acl
"#).unwrap();
    std::fs::write(inputs.path().join("restore.ps1"), r#"param([string]$Target, [string]$Saved)
$ErrorActionPreference = 'Stop'
$acl = Get-Acl -LiteralPath $Target
$acl.SetSecurityDescriptorSddlForm([System.IO.File]::ReadAllText($Saved), [System.Security.AccessControl.AccessControlSections]::Access)
Set-Acl -LiteralPath $Target -AclObject $acl
"#).unwrap();
    let guard = CreationDenied {
        directory: directory.to_owned(),
        inputs,
    };
    guard
        .apply("deny.ps1")
        .unwrap_or_else(|error| panic!("{error}"));
    guard
}

#[cfg(windows)]
impl CreationDenied {
    fn apply(&self, script: &str) -> Result<(), String> {
        let system = std::env::var_os("SystemRoot").expect("Windows system directory");
        let powershell = Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let output = std::process::Command::new(powershell)
            .args(["-NoProfile", "-NonInteractive", "-File"])
            .arg(self.inputs.path().join(script))
            .arg(&self.directory)
            .arg(self.inputs.path().join("original.sddl"))
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "fixture DACL operation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}

#[cfg(windows)]
impl Drop for CreationDenied {
    fn drop(&mut self) {
        if self.inputs.path().join("original.sddl").is_file()
            && let Err(error) = self.apply("restore.ps1")
        {
            if std::thread::panicking() {
                eprintln!("directory DACL restoration failed: {error}");
            } else {
                panic!("directory DACL restoration failed: {error}");
            }
        }
    }
}

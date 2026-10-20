use std::{fs, io, path::Path};

pub(super) enum Consent {
    Installed,
    Declined,
}

pub(super) fn register(
    bin: &Path,
    paths_file: &Path,
    state: &Path,
    reachable: bool,
    write: impl FnOnce(&Path, &str) -> io::Result<Consent>,
) -> io::Result<()> {
    if reachable {
        return Ok(());
    }
    let bin_text = bin
        .to_str()
        .filter(|text| !text.contains(['\n', '\r']))
        .ok_or_else(|| io::Error::other("bundle path cannot be represented in paths.d"))?;
    if !bin.join("artifactize").is_file() || !bin.join("artifactize-tools").is_file() {
        return Err(io::Error::other("bundle is missing its commands"));
    }
    let content = format!("{bin_text}\n");
    match fs::read_to_string(paths_file) {
        Ok(existing) if existing == content => return Ok(()),
        // A moved/deleted bundle's old entry can be repaired. Another working bundle is
        // not ours to replace, even if this shell does not use path_helper.
        Ok(existing) if Path::new(existing.trim_end()).join("artifactize").is_file() => {
            return Ok(());
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let declined_file = state.join("desktop-cli-declined.json");
    let mut declined: Vec<std::path::PathBuf> = match fs::read(&declined_file) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    if declined.iter().any(|path| path == bin) {
        return Ok(());
    }
    match write(paths_file, &content)? {
        Consent::Installed => Ok(()),
        Consent::Declined => {
            declined.push(bin.to_owned());
            fs::create_dir_all(state)?;
            fs::write(
                declined_file,
                serde_json::to_vec(&declined).map_err(io::Error::other)?,
            )
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn write_privileged(destination: &Path, content: &str) -> io::Result<Consent> {
    use std::{
        io::Write,
        process::{Command, Stdio},
        time::Duration,
    };
    let mut source = tempfile::NamedTempFile::new()?;
    source.write_all(content.as_bytes())?;
    source.flush()?;
    let mut sudo = Command::new("/usr/bin/sudo");
    sudo.args(["-n", "/usr/bin/install", "-m", "644"])
        .arg(source.path())
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if super::bounded_status(&mut sudo, Duration::from_secs(5))?.success() {
        return Ok(Consent::Installed);
    }
    // AppleScript's elevation API accepts only a shell command, not an argv. Quote
    // absolute paths for that API only; no user value is interpolated into AppleScript.
    fn quote(path: &Path) -> io::Result<String> {
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::other("non-Unicode elevation path"))?;
        Ok(format!("'{}'", text.replace('\'', "'\\''")))
    }
    let script = "on run argv\n do shell script (item 1 of argv) with prompt \"Artifactize wants to make the artifactize command available in Terminal.\" with administrator privileges\nend run";
    let shell_command = format!(
        "/usr/bin/install -m 644 {} {}",
        quote(source.path())?,
        quote(destination)?
    );
    let errors = tempfile::NamedTempFile::new()?;
    let mut prompt = Command::new("/usr/bin/osascript");
    prompt
        .args(["-e", script, &shell_command])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(errors.reopen()?);
    // The person needs time to read the system prompt and authenticate. This runs off
    // the UI thread and is still bounded if the authorization service stops answering.
    let status = super::bounded_status(&mut prompt, Duration::from_secs(120))?;
    if status.success() {
        Ok(Consent::Installed)
    } else {
        let message = fs::read_to_string(errors.path())?;
        if message.contains("(-128)") {
            Ok(Consent::Declined)
        } else {
            Err(io::Error::other(format!(
                "command registration authorization failed: {message}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_respects_existing_install_repairs_moves_and_remembers_declines() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("First.app/Contents/MacOS");
        let moved = root.path().join("Moved.app/Contents/MacOS");
        let state = root.path().join("state");
        let paths = root.path().join("paths.d/artifactize");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(paths.parent().unwrap()).unwrap();
        for command in ["artifactize", "artifactize-tools"] {
            fs::write(first.join(command), "bundled command").unwrap();
        }
        register(&first, &paths, &state, true, |_, _| {
            panic!("already reachable")
        })
        .unwrap();
        assert!(!paths.exists());
        register(&first, &paths, &state, false, |path, content| {
            fs::write(path, content)?;
            Ok(Consent::Installed)
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(&paths).unwrap(),
            format!("{}\n", first.display())
        );
        fs::create_dir_all(moved.parent().unwrap()).unwrap();
        fs::rename(&first, &moved).unwrap();
        register(&moved, &paths, &state, false, |path, content| {
            fs::write(path, content)?;
            Ok(Consent::Installed)
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(&paths).unwrap(),
            format!("{}\n", moved.display())
        );
        fs::remove_file(&paths).unwrap();
        register(&moved, &paths, &state, false, |_, _| Ok(Consent::Declined)).unwrap();
        register(&moved, &paths, &state, false, |_, _| {
            panic!("declined for this bundle")
        })
        .unwrap();
        assert!(!paths.exists());
        fs::create_dir_all(&first).unwrap();
        for command in ["artifactize", "artifactize-tools"] {
            fs::write(first.join(command), "bundled command").unwrap();
        }
        register(&first, &paths, &state, false, |path, content| {
            fs::write(path, content)?;
            Ok(Consent::Installed)
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(&paths).unwrap(),
            format!("{}\n", first.display())
        );
    }
}

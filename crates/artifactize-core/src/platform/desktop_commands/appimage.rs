use std::{fs, io, os::unix::fs::symlink, path::Path};

const COMMANDS: [&str; 2] = ["artifactize", "artifactize-tools"];

pub(super) fn register(
    bundled: &Path,
    stable: &Path,
    local_bin: &Path,
    version: &str,
    reachable: bool,
) -> io::Result<()> {
    let owns_cli = fs::read_link(local_bin.join("artifactize"))
        .is_ok_and(|target| target == stable.join("artifactize"));
    // Our stable copies must follow AppImage updates even when our own link is already
    // reachable. An unrelated existing installation always wins.
    if reachable && !owns_cli {
        return Ok(());
    }
    for name in COMMANDS {
        let link = local_bin.join(name);
        match fs::symlink_metadata(&link) {
            Ok(_) if fs::read_link(&link).is_ok_and(|target| target == stable.join(name)) => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "will not replace {}: not installed by the app",
                        link.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    fs::create_dir_all(stable)?;
    fs::create_dir_all(local_bin)?;
    let installed_version = stable.join("version");
    if fs::read_to_string(&installed_version).ok().as_deref() != Some(version)
        || COMMANDS.iter().any(|name| !stable.join(name).is_file())
    {
        for name in COMMANDS {
            // Replace by rename: a running CLI retains its old executable, and a failed
            // copy cannot truncate a working one. No symlink points into the FUSE mount.
            let temporary = tempfile::NamedTempFile::new_in(stable)?;
            fs::copy(bundled.join(name), temporary.path())?;
            temporary
                .persist(stable.join(name))
                .map_err(|error| error.error)?;
        }
        fs::write(installed_version, version)?;
    }
    for name in COMMANDS {
        let link = local_bin.join(name);
        match symlink(stable.join(name), &link) {
            Ok(()) => {}
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && fs::read_link(&link).is_ok_and(|target| target == stable.join(name)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_survive_mount_removal_follow_updates_and_preserve_foreign_commands() {
        let root = tempfile::tempdir().unwrap();
        let mount = root.path().join("mount/usr/bin");
        let stable = root.path().join("data/artifactize/bin");
        let local = root.path().join("home/.local/bin");
        fs::create_dir_all(&mount).unwrap();
        for name in COMMANDS {
            fs::write(mount.join(name), format!("{name} old")).unwrap();
        }
        register(&mount, &stable, &local, "1", true).unwrap();
        assert!(!local.exists());
        register(&mount, &stable, &local, "1", false).unwrap();
        fs::remove_dir_all(root.path().join("mount")).unwrap();
        for name in COMMANDS {
            assert_eq!(
                fs::read_to_string(local.join(name)).unwrap(),
                format!("{name} old")
            );
        }
        fs::create_dir_all(&mount).unwrap();
        for name in COMMANDS {
            fs::write(mount.join(name), format!("{name} new")).unwrap();
        }
        register(&mount, &stable, &local, "2", true).unwrap();
        for name in COMMANDS {
            assert_eq!(
                fs::read_to_string(local.join(name)).unwrap(),
                format!("{name} new")
            );
        }
        fs::remove_file(local.join("artifactize")).unwrap();
        fs::write(local.join("artifactize"), "foreign installation").unwrap();
        assert_eq!(
            register(&mount, &stable, &local, "3", false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            fs::read_to_string(local.join("artifactize")).unwrap(),
            "foreign installation"
        );
        assert_eq!(
            fs::read_to_string(local.join("artifactize-tools")).unwrap(),
            "artifactize-tools new"
        );
    }
}

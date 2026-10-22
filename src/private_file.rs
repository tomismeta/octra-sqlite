use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

#[cfg(feature = "cli")]
pub(crate) fn write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = create_new(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    sync_parent(path);
    Ok(())
}

pub(crate) fn create_temporary_near(
    path: &Path,
    fallback_name: &str,
) -> io::Result<(PathBuf, File)> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback_name);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..100u32 {
        let candidate = path.with_file_name(format!(
            ".{name}.tmp.{}.{nonce}.{attempt}",
            std::process::id()
        ));
        match create_new(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not allocate a temporary file beside {}",
            path.display()
        ),
    ))
}

pub(crate) fn atomic_replace(path: &Path, contents: &[u8]) -> io::Result<()> {
    let (temporary, mut file) = create_temporary_near(path, "octra-sqlite")?;
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)?;
        sync_parent(path);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    // Rust 1.96 already replaces files on Windows via MoveFileExW or
    // FileRenameInfoEx. Never delete the destination to retry a failed rename.
    fs::rename(source, destination)
}

pub(crate) fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_replacement_leaves_one_complete_file() {
        let root = std::env::temp_dir().join(format!(
            "octra-sqlite-private-file-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        fs::write(&path, b"old").unwrap();
        atomic_replace(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_with_missing_source_preserves_destination() {
        let root = std::env::temp_dir().join(format!(
            "octra-sqlite-private-missing-source-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("missing");
        let destination = root.join("state.json");
        fs::write(&destination, b"old").unwrap();

        let error = replace_file(&source, &destination).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read(&destination).unwrap(), b"old");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_replacement_failure_preserves_destination_and_removes_temporary() {
        let root = std::env::temp_dir().join(format!(
            "octra-sqlite-private-failed-replacement-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let destination = root.join("state");
        fs::create_dir_all(&destination).unwrap();
        let existing = destination.join("keep");
        fs::write(&existing, b"old").unwrap();

        assert!(atomic_replace(&destination, b"new").is_err());

        assert_eq!(fs::read(&existing).unwrap(), b"old");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn replacement_with_locked_source_preserves_both_files() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = std::env::temp_dir().join(format!(
            "octra-sqlite-private-locked-source-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("replacement");
        let destination = root.join("state.json");
        fs::write(&source, b"new").unwrap();
        fs::write(&destination, b"old").unwrap();
        // Deny delete sharing on the source only: removing the destination
        // would succeed, but cannot make the source renamable.
        let locked_source = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&source)
            .unwrap();

        let result = replace_file(&source, &destination);
        drop(locked_source);

        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), b"new");
        assert_eq!(fs::read(&destination).unwrap(), b"old");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replacement_does_not_follow_destination_symlinks() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "octra-sqlite-private-symlink-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let victim = root.join("victim");
        let destination = root.join("state");
        fs::write(&victim, b"keep").unwrap();
        symlink(&victim, &destination).unwrap();
        atomic_replace(&destination, b"new").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(
            !fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_dir_all(root).unwrap();
    }
}

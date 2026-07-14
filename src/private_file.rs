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

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) if destination.exists() => {
            fs::remove_file(destination)?;
            fs::rename(source, destination)
        }
        Err(error) => Err(error),
    }
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

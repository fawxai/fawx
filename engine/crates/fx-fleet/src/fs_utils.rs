use crate::token::FleetError;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub(crate) const PRIVATE_FILE_MODE: u32 = 0o600;

pub(crate) fn set_private_permissions(path: &Path) -> Result<(), FleetError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    Ok(())
}

pub(crate) fn write_private(path: &Path, data: &[u8]) -> Result<(), FleetError> {
    ensure_parent_dir(path)?;
    let mut file = open_private_file(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    set_private_permissions(path)
}

pub(crate) fn write_json_private<T>(path: &Path, value: &T) -> Result<(), FleetError>
where
    T: Serialize + ?Sized,
{
    let mut json = serde_json::to_vec_pretty(value)?;
    json.push(b'\n');
    write_private(path, &json)
}

fn ensure_parent_dir(path: &Path) -> Result<(), FleetError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn open_private_file(path: &Path) -> Result<std::fs::File, FleetError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);
    Ok(options.open(path)?)
}

#[cfg(test)]
pub(crate) fn assert_private_permissions(path: &Path) {
    #[cfg(unix)]
    {
        let metadata = fs::metadata(path).expect("metadata should load");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, PRIVATE_FILE_MODE);
    }

    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_private_creates_parent_directories() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let path = temp_dir.path().join("nested/fleet.key");

        write_private(&path, b"secret").expect("private file should write");

        assert_eq!(
            fs::read(&path).expect("private file should read"),
            b"secret"
        );
    }

    #[test]
    fn write_private_sets_private_permissions() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let path = temp_dir.path().join("fleet.key");

        write_private(&path, b"secret").expect("private file should write");

        assert_private_permissions(&path);
    }
}

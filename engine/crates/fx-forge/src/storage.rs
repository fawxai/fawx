use crate::ForgeError;
use std::path::Path;

pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<(), ForgeError> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
};

fn current_dir_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Restores the original process working directory when dropped.
pub struct CurrentDirGuard {
    _lock: MutexGuard<'static, ()>,
    previous: PathBuf,
}

impl CurrentDirGuard {
    pub fn set(path: &Path) -> std::io::Result<Self> {
        let lock = match current_dir_lock().lock() {
            Ok(lock) => lock,
            Err(poisoned) => poisoned.into_inner(),
        };
        let previous = std::env::current_dir()?;
        std::env::set_current_dir(path)?;
        Ok(Self {
            _lock: lock,
            previous,
        })
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        if let Err(error) = std::env::set_current_dir(&self.previous) {
            panic!("restore current dir: {error}");
        }
    }
}

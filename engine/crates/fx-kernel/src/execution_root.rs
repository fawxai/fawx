use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Debug)]
pub struct ExecutionRoot {
    path: RwLock<PathBuf>,
}

pub type SharedExecutionRoot = Arc<ExecutionRoot>;

impl ExecutionRoot {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path: RwLock::new(path),
        }
    }

    #[must_use]
    pub fn current(&self) -> PathBuf {
        self.path
            .read()
            .unwrap_or_else(|error| recover_poisoned_guard("read", error))
            .clone()
    }

    pub fn replace(&self, path: PathBuf) -> PathBuf {
        let mut guard = self
            .path
            .write()
            .unwrap_or_else(|error| recover_poisoned_guard("write", error));
        std::mem::replace(&mut *guard, path)
    }
}

fn recover_poisoned_guard<T>(operation: &str, error: PoisonError<T>) -> T {
    tracing::warn!("execution root lock poisoned during {operation}; recovering inner state");
    error.into_inner()
}

#[cfg(test)]
mod tests {
    use super::ExecutionRoot;
    use std::path::PathBuf;

    #[test]
    fn current_recovers_after_poisoned_lock() {
        let root = ExecutionRoot::new(PathBuf::from("/tmp/original"));

        let _ = std::panic::catch_unwind(|| {
            let _guard = root.path.write().expect("write lock");
            panic!("poison lock");
        });

        assert_eq!(root.current(), PathBuf::from("/tmp/original"));
    }

    #[test]
    fn replace_recovers_after_poisoned_lock() {
        let root = ExecutionRoot::new(PathBuf::from("/tmp/original"));

        let _ = std::panic::catch_unwind(|| {
            let _guard = root.path.write().expect("write lock");
            panic!("poison lock");
        });

        let previous = root.replace(PathBuf::from("/tmp/updated"));
        assert_eq!(previous, PathBuf::from("/tmp/original"));
        assert_eq!(root.current(), PathBuf::from("/tmp/updated"));
    }
}

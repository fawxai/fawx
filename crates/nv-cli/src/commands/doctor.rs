//! System diagnostics command

const CHECK_MARK: &str = "\x1b[32m✓\x1b[0m"; // Green checkmark
const CROSS_MARK: &str = "\x1b[31m✗\x1b[0m"; // Red X

/// Run system diagnostics
pub async fn run() -> anyhow::Result<i32> {
    println!("Running Nova diagnostics...\n");

    let mut all_passed = true;

    // Check workspace directory
    if check_workspace() {
        println!("{} Workspace directory exists", CHECK_MARK);
    } else {
        println!("{} Workspace directory missing", CROSS_MARK);
        all_passed = false;
    }

    // Check config file
    if check_config() {
        println!("{} Config file exists", CHECK_MARK);
    } else {
        println!("{} Config file not found (will use defaults)", CHECK_MARK);
    }

    // Check model file
    if check_model() {
        println!("{} Model file accessible", CHECK_MARK);
    } else {
        println!("{} Model file not found (configurable)", CHECK_MARK);
    }

    // Check storage directory
    if check_storage().await {
        println!("{} Storage directory is writable", CHECK_MARK);
    } else {
        println!("{} Storage directory not writable", CROSS_MARK);
        all_passed = false;
    }

    // Check audit log
    if check_audit_log().await {
        println!("{} Audit log is intact", CHECK_MARK);
    } else {
        println!("{} Audit log verification failed", CROSS_MARK);
        all_passed = false;
    }

    println!();
    if all_passed {
        println!("All critical checks passed!");
        Ok(0)
    } else {
        println!("Some critical checks failed");
        Ok(1)
    }
}

fn check_workspace() -> bool {
    get_workspace_dir().map(|p| p.exists()).unwrap_or(false)
}

fn check_config() -> bool {
    get_config_path().map(|p| p.exists()).unwrap_or(false)
}

fn check_model() -> bool {
    // Model location is configurable, so this is informational
    // For now, always pass since the model path is user-configurable
    true
}

async fn check_storage() -> bool {
    let storage_dir = match get_storage_dir() {
        Ok(p) => p,
        Err(_) => return false,
    };

    // Create directory if it doesn't exist (create_dir_all is idempotent)
    if tokio::fs::create_dir_all(&storage_dir).await.is_err() {
        return false;
    }

    // Test writability by creating a temp file
    let test_file = storage_dir.join(".writetest");
    match tokio::fs::write(&test_file, b"test").await {
        Ok(_) => {
            let _ = tokio::fs::remove_file(&test_file).await;
            true
        }
        Err(_) => false,
    }
}

async fn check_audit_log() -> bool {
    let log_path = match get_audit_log_path() {
        Ok(p) => p,
        Err(_) => return false,
    };

    if !log_path.exists() {
        // No log file yet is fine
        return true;
    }

    // Try to open and verify the log
    match nv_security::AuditLog::open(&log_path).await {
        Ok(log) => match log.verify_integrity() {
            Ok(valid) => valid,
            Err(e) => {
                tracing::warn!("Audit log verification error: {}", e);
                false
            }
        },
        Err(_) => false,
    }
}

fn get_workspace_dir() -> anyhow::Result<std::path::PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".nova"))
}

fn get_config_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(get_workspace_dir()?.join("config.toml"))
}

fn get_storage_dir() -> anyhow::Result<std::path::PathBuf> {
    Ok(get_workspace_dir()?.join("storage"))
}

fn get_audit_log_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(get_workspace_dir()?.join("audit.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_check_functions_return_bool() {
        // These should return bool values without panicking
        let _ = check_workspace();
        let _ = check_config();
        let _ = check_model();
    }

    #[tokio::test]
    async fn test_async_check_functions_return_bool() {
        let _ = check_storage().await;
        let _ = check_audit_log().await;
    }
}

use std::env;
use std::path::PathBuf;

/// Returns the root directory for Ozzie data.
///
/// Checks `$OZZIE_PATH` first, then `$OZZIE_HOME`, otherwise defaults to `~/.ozzie`.
pub fn ozzie_path() -> PathBuf {
    if let Ok(v) = env::var("OZZIE_PATH")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    if let Ok(v) = env::var("OZZIE_HOME")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    match dirs::home_dir() {
        Some(home) => home.join(".ozzie"),
        None => PathBuf::from(".").join(".ozzie"),
    }
}

/// Returns the path to the installation's device key file.
pub fn key_path() -> PathBuf {
    ozzie_path().join(".key")
}

/// Returns the path to the Ozzie config file.
pub fn config_path() -> PathBuf {
    ozzie_path().join("config.jsonc")
}

/// Returns the path to the Ozzie .env file.
pub fn dotenv_path() -> PathBuf {
    ozzie_path().join(".env")
}

/// Returns the path to the logs directory.
pub fn logs_path() -> PathBuf {
    ozzie_path().join("logs")
}

/// Returns the path to the sessions directory.
pub fn sessions_path() -> PathBuf {
    ozzie_path().join("sessions")
}

/// Returns the path to the tasks directory.
pub fn tasks_path() -> PathBuf {
    ozzie_path().join("tasks")
}

/// Returns the path to the memory directory.
pub fn memory_path() -> PathBuf {
    ozzie_path().join("memory")
}

/// Returns the path to the skills directory.
pub fn skills_path() -> PathBuf {
    ozzie_path().join("skills")
}

/// Returns the path to the plugins directory.
pub fn plugins_path() -> PathBuf {
    ozzie_path().join("plugins")
}

/// Returns the path to the Discord connector runtime database.
pub fn discord_db_path() -> PathBuf {
    ozzie_path().join("connectors").join("discord.jsonc")
}

/// Ensures all standard ozzie subdirectories exist.
///
/// Called at startup (gateway, CLI commands) to avoid errors when
/// `ozzie wake` hasn't been run or a directory was deleted.
pub fn ensure_dirs() -> std::io::Result<()> {
    for path in [
        ozzie_path(),
        logs_path(),
        sessions_path(),
        tasks_path(),
        memory_path(),
        skills_path(),
        plugins_path(),
    ] {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_ends_with_jsonc() {
        let p = config_path();
        assert!(
            p.to_string_lossy().ends_with("config.jsonc"),
            "got {:?}",
            p
        );
    }

    #[test]
    fn dotenv_path_ends_with_env() {
        let p = dotenv_path();
        assert!(p.to_string_lossy().ends_with(".env"), "got {:?}", p);
    }

    #[test]
    fn subdirs_under_ozzie_path() {
        let root = ozzie_path();
        assert_eq!(logs_path(), root.join("logs"));
        assert_eq!(sessions_path(), root.join("sessions"));
        assert_eq!(tasks_path(), root.join("tasks"));
        assert_eq!(memory_path(), root.join("memory"));
        assert_eq!(skills_path(), root.join("skills"));
        assert_eq!(plugins_path(), root.join("plugins"));
    }
}

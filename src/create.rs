use crate::config::Config;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use ulid::Ulid;

#[derive(Serialize, Deserialize)]
pub struct RepositoryMetadata {
    pub branch: String,
    pub created_at: chrono::DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

pub fn read_ulid_metadata(path: &Path) -> Result<RepositoryMetadata, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;

    if let Ok(json_content) = serde_json::from_str::<RepositoryMetadata>(&content) {
        return Ok(json_content);
    }

    let branch = content.trim().to_string();
    let directory = branch_to_directory_name(&branch);
    Ok(RepositoryMetadata {
        branch: branch.clone(),
        created_at: Utc::now(),
        directory: Some(directory),
    })
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_path = if let Ok(path) = std::env::var("TRR_CONFIG_PATH") {
        expand_tilde(&path)
    } else {
        dirs::home_dir()
            .expect("Failed to get home directory")
            .join(".config")
            .join("trr")
            .join("config.toml")
    };

    if !config_path.exists() {
        return Ok(Config::default());
    }

    let config_str = fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&config_str)?;
    Ok(config)
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn expand_alias(branch: &str, config: &Config) -> String {
    for (alias, expansion) in &config.branch_aliases {
        if branch.starts_with(alias) {
            let suffix = &branch[alias.len()..];
            if let Some(cmd) = expansion.strip_prefix('!') {
                if let Ok(output) = Command::new("sh").arg("-c").arg(cmd).output() {
                    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    return format!("{result}{suffix}");
                }
            } else {
                return format!("{expansion}{suffix}");
            }
        }
    }
    branch.to_string()
}

pub fn branch_to_directory_name(branch: &str) -> String {
    branch.replace('/', "-")
}

fn check_tmux_available() -> bool {
    Command::new("which")
        .arg("tmux")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn get_repo_name() -> Option<String> {
    let output = Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let repo_name = if url.starts_with("https://") || url.starts_with("http://") {
        url.split('/')
            .next_back()?
            .trim_end_matches(".git")
            .to_string()
    } else if url.contains(':') {
        url.split(':')
            .next_back()?
            .split('/')
            .next_back()?
            .trim_end_matches(".git")
            .to_string()
    } else {
        return None;
    };

    Some(repo_name)
}

fn get_repo_prefix() -> String {
    if let Some(repo_name) = get_repo_name() {
        repo_name.chars().take(3).collect()
    } else {
        std::env::current_dir()
            .ok()
            .and_then(|dir| {
                dir.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .map(|name| name.chars().take(3).collect())
            .unwrap_or_else(|| "trr".to_string())
    }
}

fn setup_tmux_environment(
    branch_name: &str,
    target_dir: &Path,
    init_commands: &str,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if !check_tmux_available() {
        eprintln!("Warning: tmux is not installed. Skipping tmux setup.");
        eprintln!("To use tmux integration, please install tmux.");
        return Ok(());
    }

    let repo_prefix = get_repo_prefix();

    let args_str = args.join(" ");
    let processed_commands = init_commands.replace("@@args", &args_str);

    let in_tmux = std::env::var("TMUX").is_ok();

    if in_tmux {
        let window_name = format!("{repo_prefix}-{branch_name}");

        println!("Creating new tmux window '{window_name}' in current session...");
        let create_window = Command::new("tmux")
            .arg("new-window")
            .arg("-n")
            .arg(&window_name)
            .arg("-c")
            .arg(target_dir.to_string_lossy().to_string())
            .output()?;

        if !create_window.status.success() {
            eprintln!(
                "Failed to create tmux window. stderr: {}",
                String::from_utf8_lossy(&create_window.stderr)
            );
            return Err("Failed to create tmux window".into());
        }

        if !processed_commands.trim().is_empty() {
            for command in processed_commands.trim().lines() {
                if !command.trim().is_empty() {
                    Command::new("tmux")
                        .arg("send-keys")
                        .arg("-t")
                        .arg(&window_name)
                        .arg(command)
                        .arg("Enter")
                        .status()?;
                }
            }
        }

        Command::new("tmux")
            .arg("select-window")
            .arg("-t")
            .arg(&window_name)
            .status()?;

        println!("✓ Switched to new window '{window_name}'");
    } else if std::io::stdin().is_terminal() {
        let session_name = format!("{repo_prefix}-{branch_name}");

        println!(
            "Creating tmux session '{}' in directory '{}'",
            session_name,
            target_dir.display()
        );
        let create_result = Command::new("tmux")
            .arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(&session_name)
            .arg("-c")
            .arg(target_dir.to_string_lossy().to_string())
            .output()?;

        if !create_result.status.success() {
            eprintln!(
                "Failed to create tmux session. stderr: {}",
                String::from_utf8_lossy(&create_result.stderr)
            );
            return Err("Failed to create tmux session".into());
        }

        if !processed_commands.trim().is_empty() {
            for command in processed_commands.trim().lines() {
                if !command.trim().is_empty() {
                    Command::new("tmux")
                        .arg("send-keys")
                        .arg("-t")
                        .arg(&session_name)
                        .arg(command)
                        .arg("Enter")
                        .status()?;
                }
            }
        }

        println!("Attaching to tmux session '{session_name}'...");
        Command::new("tmux")
            .arg("attach-session")
            .arg("-t")
            .arg(&session_name)
            .status()?;
    } else {
        println!(
            "Not in a terminal environment. Navigate to {} to start working.",
            target_dir.display()
        );
    }

    Ok(())
}

pub fn create_repo(
    branch: &str,
    args: &[String],
    debug: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config()?;
    let expanded_branch = expand_alias(branch, &config);
    let directory_name = branch_to_directory_name(&expanded_branch);

    if debug {
        eprintln!("Debug: Branch alias expansion: {branch} -> {expanded_branch}");
        eprintln!("Debug: Directory name: {directory_name}");
    }

    let target_dir = PathBuf::from(&config.settings.repo_sync_path).join(&directory_name);
    if target_dir.exists() {
        return Err(format!(
            "Directory '{}' already exists. Use a different branch name or delete the existing one first.",
            target_dir.display()
        ).into());
    }

    let ulid = Ulid::new();

    let current_dir = std::env::current_dir()?;

    let trr_sys_path = PathBuf::from(&config.settings.repo_sync_path).join(".trr-sys");
    fs::create_dir_all(&trr_sys_path)?;

    let metadata = RepositoryMetadata {
        branch: expanded_branch.clone(),
        created_at: Utc::now(),
        directory: Some(directory_name.clone()),
    };
    let ulid_file_path = trr_sys_path.join(format!("{ulid}.json"));
    let json_content = serde_json::to_string_pretty(&metadata)?;
    fs::write(&ulid_file_path, json_content)?;

    fs::create_dir_all(&target_dir)?;

    let mut rsync_command = Command::new("rsync");
    rsync_command.arg("-a");

    if debug {
        rsync_command.arg("-v");
    }

    // Always exclude repo_sync_path
    rsync_command
        .arg("--exclude")
        .arg(&config.settings.repo_sync_path);

    // Add user-defined excludes
    for exclude in &config.settings.rsync_excludes {
        rsync_command.arg("--exclude").arg(exclude);
    }

    let rsync_result = rsync_command
        .arg(format!("{}/", current_dir.display()))
        .arg(format!("{}/", target_dir.display()))
        .status()?;

    if !rsync_result.success() {
        return Err("rsync failed".into());
    }

    let absolute_target_dir = current_dir.join(&target_dir);
    let checkout_result = Command::new("git")
        .arg("checkout")
        .arg("-b")
        .arg(&expanded_branch)
        .current_dir(&absolute_target_dir)
        .output()?;

    if !checkout_result.status.success() {
        eprintln!(
            "Failed to create git branch. stderr: {}",
            String::from_utf8_lossy(&checkout_result.stderr)
        );
        return Err("Failed to create git branch".into());
    }

    println!("Repository duplicated successfully:");
    println!("  Branch: {branch} -> {expanded_branch}");
    println!("  ULID: {ulid}");
    println!("  Target: {}", target_dir.display());

    setup_tmux_environment(
        &expanded_branch,
        &absolute_target_dir,
        &config.settings.tmux_window_init_commands,
        args,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_branch_to_directory_name() {
        assert_eq!(branch_to_directory_name("feature/test"), "feature-test");
        assert_eq!(branch_to_directory_name("fix/bug/123"), "fix-bug-123");
        assert_eq!(branch_to_directory_name("simple-branch"), "simple-branch");
        assert_eq!(branch_to_directory_name(""), "");
    }

    #[test]
    fn test_expand_alias_static() {
        let mut config = Config::default();
        config.branch_aliases.clear();
        config
            .branch_aliases
            .insert("@f".to_string(), "feature".to_string());
        config
            .branch_aliases
            .insert("@b".to_string(), "bugfix".to_string());

        assert_eq!(expand_alias("@f/test", &config), "feature/test");
        assert_eq!(expand_alias("@b/123", &config), "bugfix/123");
        assert_eq!(expand_alias("@f", &config), "feature");
        assert_eq!(expand_alias("no-alias", &config), "no-alias");
    }

    #[test]
    fn test_read_ulid_metadata_json() {
        let temp_dir = std::env::temp_dir();
        let test_id = Ulid::new().to_string();
        let file_path = temp_dir.join(format!("trr_test_{test_id}.json"));

        let metadata = RepositoryMetadata {
            branch: "feature/test".to_string(),
            created_at: Utc::now(),
            directory: Some("feature-test".to_string()),
        };

        let json = serde_json::to_string_pretty(&metadata).unwrap();
        fs::write(&file_path, json).unwrap();

        let result = read_ulid_metadata(&file_path).unwrap();
        assert_eq!(result.branch, "feature/test");
        assert_eq!(result.directory, Some("feature-test".to_string()));

        // Clean up
        let _ = fs::remove_file(&file_path);
    }

    #[test]
    fn test_read_ulid_metadata_plain_text() {
        let temp_dir = std::env::temp_dir();
        let test_id = Ulid::new().to_string();
        let file_path = temp_dir.join(format!("trr_test_{test_id}.txt"));

        fs::write(&file_path, "feature/legacy").unwrap();

        let result = read_ulid_metadata(&file_path).unwrap();
        assert_eq!(result.branch, "feature/legacy");
        assert_eq!(result.directory, Some("feature-legacy".to_string()));

        // Clean up
        let _ = fs::remove_file(&file_path);
    }

    #[test]
    fn test_check_tmux_available() {
        // This test just ensures the function runs without panic
        let _ = check_tmux_available();
    }

    #[test]
    fn test_get_repo_prefix() {
        // This test ensures the function runs and returns a string
        let prefix = get_repo_prefix();
        assert!(!prefix.is_empty());
        assert!(prefix.len() <= 3);
    }
}

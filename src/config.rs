use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub settings: Settings,
    pub branch_aliases: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub repo_sync_path: String,
    pub tmux_window_init_commands: String,
    pub rsync_excludes: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut branch_aliases = HashMap::new();
        branch_aliases.insert("@f".to_string(), "feature".to_string());
        branch_aliases.insert("@b".to_string(), "bugfix".to_string());
        branch_aliases.insert(
            "@t".to_string(),
            "!echo feature/$(date +%Y%m%d-%H%M%S)".to_string(),
        );

        Config {
            settings: Settings {
                repo_sync_path: ".trr".to_string(),
                tmux_window_init_commands: r#"git reset --hard
tmux split-window -h
tmux split-window -v -t 1
tmux send-keys -t 2 'lazygit' C-m
tmux send-keys -t 1 'if [ -n "@@args" ]; then claude --dangerously-skip-permissions "@@args"; else claude; fi' C-m
tmux send-keys -t 0 'nvim' C-m
tmux select-pane -t 1
"#
                .to_string(),
                rsync_excludes: vec![
                    "target".to_string()
                ],
            },
            branch_aliases,
        }
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn get_config_path() -> PathBuf {
    if let Ok(config_path) = env::var("TRR_CONFIG_PATH") {
        expand_tilde(&config_path)
    } else {
        dirs::home_dir()
            .expect("Failed to get home directory")
            .join(".config")
            .join("trr")
            .join("config.toml")
    }
}

fn get_editor() -> Option<String> {
    env::var("TRR_EDITOR")
        .or_else(|_| env::var("EDITOR"))
        .or_else(|_| env::var("VISUAL"))
        .ok()
}

pub fn init_config() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = get_config_path();

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if !config_path.exists() {
        let default_config = Config::default();
        let toml_string = toml::to_string_pretty(&default_config)?;
        fs::write(&config_path, toml_string)?;
        println!("Created new config file at: {}", config_path.display());
    }

    if let Some(editor) = get_editor() {
        println!("Opening config file with {editor}...");
        Command::new(editor).arg(&config_path).status()?;
    } else {
        println!("No editor found in TRR_EDITOR, EDITOR, or VISUAL environment variables");
        println!("Config file location: {}", config_path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_with_home() {
        let path = "~/test/path";
        let result = expand_tilde(path);

        if let Some(home) = dirs::home_dir() {
            assert_eq!(result, home.join("test/path"));
        }
    }

    #[test]
    fn test_expand_tilde_without_tilde() {
        let path = "/absolute/path";
        let result = expand_tilde(path);
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_expand_tilde_relative_path() {
        let path = "relative/path";
        let result = expand_tilde(path);
        assert_eq!(result, PathBuf::from("relative/path"));
    }

    #[test]
    fn test_get_editor_priority() {
        unsafe {
            std::env::remove_var("TRR_EDITOR");
            std::env::remove_var("EDITOR");
            std::env::remove_var("VISUAL");
        }

        assert_eq!(get_editor(), None);

        unsafe {
            std::env::set_var("VISUAL", "vim");
        }
        assert_eq!(get_editor(), Some("vim".to_string()));

        unsafe {
            std::env::set_var("EDITOR", "nano");
        }
        assert_eq!(get_editor(), Some("nano".to_string()));

        unsafe {
            std::env::set_var("TRR_EDITOR", "emacs");
        }
        assert_eq!(get_editor(), Some("emacs".to_string()));

        unsafe {
            std::env::remove_var("TRR_EDITOR");
            std::env::remove_var("EDITOR");
            std::env::remove_var("VISUAL");
        }
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();

        assert_eq!(config.settings.repo_sync_path, ".trr");
        assert!(!config.settings.tmux_window_init_commands.is_empty());
        assert!(
            config
                .settings
                .rsync_excludes
                .contains(&"target".to_string())
        );

        assert_eq!(
            config.branch_aliases.get("@f"),
            Some(&"feature".to_string())
        );
        assert_eq!(config.branch_aliases.get("@b"), Some(&"bugfix".to_string()));
        assert!(
            config
                .branch_aliases
                .get("@t")
                .unwrap()
                .starts_with("!echo")
        );
    }
}

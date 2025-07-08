use crate::config::Config;
use crate::create::read_ulid_metadata;
use chrono::{DateTime, Utc};
use skim::prelude::*;
use std::borrow::Cow;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

#[derive(Clone)]
struct Repository {
    _ulid: String,
    branch: String,
    directory: String,
    path: PathBuf,
    created_at: DateTime<Utc>,
}

impl SkimItem for Repository {
    fn text(&self) -> Cow<str> {
        let formatted_date = self.created_at.format("%Y-%m-%d %H:%M:%S");
        Cow::Owned(format!("{}\t{}", formatted_date, self.branch))
    }
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

fn get_repositories(config: &Config) -> Result<Vec<Repository>, Box<dyn std::error::Error>> {
    let trr_sys_path = PathBuf::from(&config.settings.repo_sync_path).join(".trr-sys");

    if !trr_sys_path.exists() {
        return Ok(Vec::new());
    }

    let mut repositories = Vec::new();

    for entry in fs::read_dir(trr_sys_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            let ulid = file_name.strip_suffix(".json").unwrap_or(file_name);

            if let Ok(metadata) = read_ulid_metadata(&path) {
                let directory = metadata
                    .directory
                    .unwrap_or_else(|| crate::create::branch_to_directory_name(&metadata.branch));
                repositories.push(Repository {
                    _ulid: ulid.to_string(),
                    branch: metadata.branch.clone(),
                    directory,
                    path,
                    created_at: metadata.created_at,
                });
            }
        }
    }

    repositories.sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(repositories)
}

fn select_repository_with_skim(
    repositories: &[Repository],
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    if repositories.is_empty() {
        println!("No repositories found.");
        return Ok(None);
    }

    let options = SkimOptionsBuilder::default()
        .height("50%".to_string())
        .prompt("Select repository> ".to_string())
        .layout("reverse".to_string())
        .build()
        .unwrap();

    let items: Vec<(usize, Arc<dyn SkimItem>)> = repositories
        .iter()
        .enumerate()
        .map(|(idx, repo)| (idx, Arc::new(repo.clone()) as Arc<dyn SkimItem>))
        .collect();

    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    for (_, item) in &items {
        let _ = tx.send(item.clone());
    }
    drop(tx);

    let selected = Skim::run_with(&options, Some(rx));

    if let Some(output) = selected {
        if output.is_abort {
            return Ok(None);
        }

        if let Some(selected_item) = output.selected_items.first() {
            for (original_idx, item) in &items {
                if Arc::ptr_eq(item, selected_item) {
                    return Ok(Some(*original_idx));
                }
            }
        }
    }

    Ok(None)
}

fn find_tmux_session_or_window(branch: &str) -> Option<(String, bool)> {
    let repo_prefix = get_repo_prefix();
    let name = format!("{repo_prefix}-{branch}");

    let in_tmux = std::env::var("TMUX").is_ok();

    if in_tmux {
        let output = Command::new("tmux")
            .arg("list-windows")
            .arg("-F")
            .arg("#{window_name}")
            .output()
            .ok()?;

        let windows = String::from_utf8_lossy(&output.stdout);
        for window in windows.lines() {
            if window == name {
                return Some((name, true));
            }
        }
    }

    let output = Command::new("tmux")
        .arg("list-sessions")
        .arg("-F")
        .arg("#{session_name}")
        .output()
        .ok()?;

    let sessions = String::from_utf8_lossy(&output.stdout);
    for session in sessions.lines() {
        if session == name {
            return Some((name, false));
        }
    }

    None
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

fn kill_tmux_session_or_window(
    name: &str,
    is_window: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_window {
        Command::new("tmux")
            .arg("kill-window")
            .arg("-t")
            .arg(name)
            .status()?;
    } else {
        Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(name)
            .status()?;
    }
    Ok(())
}

pub fn delete_repo() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config()?;
    let repositories = get_repositories(&config)?;

    if let Some(index) = select_repository_with_skim(&repositories)? {
        let repo = &repositories[index];

        Command::new("clear").status().ok();

        println!("Selected repository: {}", repo.branch);
        println!(
            "Created at: {}",
            repo.created_at.format("%Y-%m-%d %H:%M:%S")
        );
        println!();

        print!("Are you sure you want to delete this repository? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase() != "y" {
            println!("Deletion cancelled.");
            return Ok(());
        }

        if let Some((tmux_name, is_window)) = find_tmux_session_or_window(&repo.branch) {
            println!(
                "Killing tmux {}: {}",
                if is_window { "window" } else { "session" },
                tmux_name
            );
            kill_tmux_session_or_window(&tmux_name, is_window)?;
        }

        let repo_dir = PathBuf::from(&config.settings.repo_sync_path).join(&repo.directory);
        if repo_dir.exists() {
            println!("Removing directory: {}", repo_dir.display());
            fs::remove_dir_all(&repo_dir)?;
        }

        fs::remove_file(&repo.path)?;

        println!("Successfully deleted repository '{}'", repo.branch);
    } else {
        println!("No repository selected.");
    }

    Ok(())
}

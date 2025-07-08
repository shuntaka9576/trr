use clap::{Parser, Subcommand};

mod config;
mod create;
mod delete;

const APP_VERSION: &str = concat!(
    env!("CARGO_PKG_NAME"),
    " version ",
    env!("CARGO_PKG_VERSION"),
    " (rev:",
    env!("GIT_HASH"),
    ")"
);

#[derive(Parser)]
#[command(name = "trr")]
#[command(about = "tmux-repo-rsync - Repository duplication and tmux environment management tool", long_about = None)]
#[command(disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long, short = 'V', help = "Print version")]
    version: bool,
}

#[derive(Subcommand)]
enum Commands {
    #[command(alias = "c")]
    #[command(
        about = "Create a new repository copy using rsync and set up a tmux session/window (alias: c)"
    )]
    Create {
        branch: String,

        #[arg(trailing_var_arg = true)]
        #[arg(help = "Arguments to pass to tmux initialization commands")]
        args: Vec<String>,

        #[arg(long, help = "Enable debug output including rsync verbose logs")]
        debug: bool,
    },

    #[command(about = "Open the config file in your editor or create it with defaults (no alias)")]
    Config,

    #[command(alias = "d")]
    #[command(about = "Select and delete repository copies using fuzzy search (alias: d)")]
    Delete,
}

fn main() {
    let cli = Cli::parse();

    if cli.version {
        println!("{APP_VERSION}");
        std::process::exit(0);
    }

    match cli.command {
        Some(command) => match command {
            Commands::Create {
                branch,
                args,
                debug,
            } => {
                if let Err(e) = create::create_repo(&branch, &args, debug) {
                    eprintln!("Error creating repository: {e}");
                    std::process::exit(1);
                }
            }
            Commands::Config => {
                if let Err(e) = config::init_config() {
                    eprintln!("Error initializing config: {e}");
                    std::process::exit(1);
                }
            }
            Commands::Delete => {
                if let Err(e) = delete::delete_repo() {
                    eprintln!("Error deleting repository: {e}");
                    std::process::exit(1);
                }
            }
        },
        None => {
            eprintln!("No command specified. Use --help for usage information.");
            std::process::exit(1);
        }
    }
}

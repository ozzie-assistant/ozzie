mod input;
mod markdown;
mod repl;
mod spinner;

use clap::Args;

/// Interactive REPL — connect to the gateway and chat.
#[derive(Args)]
pub struct ChatArgs {
    /// Gateway URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,

    /// Session ID to resume.
    #[arg(short, long)]
    session: Option<String>,

    /// Accept all dangerous tools automatically.
    #[arg(short = 'y', long)]
    accept_all: bool,

    /// Working directory for the session.
    #[arg(long)]
    working_dir: Option<String>,
}

impl Default for ChatArgs {
    fn default() -> Self {
        Self {
            gateway: "http://127.0.0.1:18420".to_string(),
            session: None,
            accept_all: false,
            working_dir: None,
        }
    }
}

pub use repl::run;

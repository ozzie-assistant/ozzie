use clap::Parser;
use ozzie_tui::TuiOpts;

#[derive(Parser)]
#[command(name = "ozzie-tui", about = "Ozzie — terminal UI")]
struct Args {
    /// Gateway URL (HTTP base, WebSocket derived automatically).
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,

    /// Conversation ID to resume.
    #[arg(short, long)]
    session: Option<String>,

    /// Working directory override for the session.
    #[arg(long)]
    working_dir: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    ozzie_tui::run(TuiOpts {
        gateway: &args.gateway,
        session: args.session.as_deref(),
        working_dir: args.working_dir.as_deref(),
    })
    .await
}

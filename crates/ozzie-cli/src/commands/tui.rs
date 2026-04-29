use clap::Args;
use ozzie_tui::TuiOpts;

/// Lance l'interface TUI interactive.
#[derive(Args)]
pub struct TuiArgs {
    /// Gateway URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    pub gateway: String,

    /// Conversation ID à reprendre.
    #[arg(short, long)]
    pub session: Option<String>,

    /// Répertoire de travail pour la session.
    #[arg(long)]
    pub working_dir: Option<String>,
}

pub async fn run(args: TuiArgs) -> anyhow::Result<()> {
    ozzie_tui::run(TuiOpts {
        gateway: &args.gateway,
        session: args.session.as_deref(),
        working_dir: args.working_dir.as_deref(),
    })
    .await
}

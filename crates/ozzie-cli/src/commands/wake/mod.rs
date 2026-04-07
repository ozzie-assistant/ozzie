mod acquaintance;
mod i18n;
mod orchestrator;
mod wizard;

use clap::Args;

/// Interactive setup wizard.
#[derive(Args)]
pub struct WakeArgs {
    /// Skip prompts and use defaults.
    #[arg(long)]
    pub defaults: bool,
}

pub use wizard::run;

mod app;
mod client;
mod composer;
mod events;
mod overlays;
mod render;
mod status;
mod streaming;
mod transcript;
mod tui;

use anyhow::Context;
use ozzie_client::{OzzieClient, OpenConversationOpts};
use ozzie_utils::config::ozzie_path;
use tokio::sync::mpsc;

use app::App;
use client::spawn_bridge;

pub struct TuiOpts<'a> {
    pub gateway: &'a str,
    pub session: Option<&'a str>,
    pub working_dir: Option<&'a str>,
}

pub async fn run(opts: TuiOpts<'_>) -> anyhow::Result<()> {
    let token = OzzieClient::acquire_token_cli(opts.gateway, &ozzie_path())
        .await
        .context("acquire token")?;

    let mut client = OzzieClient::connect(opts.gateway, Some(&token))
        .await
        .context("connect to gateway")?;

    let conversation_id = client
        .open_session(OpenConversationOpts {
            conversation_id: opts.session,
            working_dir: opts.working_dir,
        })
        .await
        .context("open session")?;

    let (server_tx, server_rx) = mpsc::unbounded_channel();
    let out_tx = spawn_bridge(client, server_tx);

    let mut terminal = tui::init()?;
    let result = App::new(conversation_id, server_rx, out_tx).run(&mut terminal).await;
    tui::restore(&mut terminal)?;
    result
}

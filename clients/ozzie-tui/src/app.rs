use std::time::Duration;

use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use ratatui::{Frame, layout::{Constraint, Direction, Layout}};
use tokio::sync::mpsc;

use crate::client::{OutboundMsg, ServerEvent};
use crate::composer::{ComposerOutput, ComposerWidget};
use crate::events::AppEvent;
use crate::overlays::{ApprovalOverlay, Overlay, ViewAction};
use crate::status::StatusLine;
use crate::streaming::StreamController;
use crate::transcript::{ApprovalCell, AssistantCell, ToolCell, TranscriptWidget, UserCell};
use crate::tui::Tui;

const TICK_MS: u64 = 80; // ~12 fps pour les animations (spinner, shimmer)

pub struct App {
    session_id: String,
    transcript: TranscriptWidget,
    composer: ComposerWidget,
    overlay_stack: Vec<Box<dyn Overlay>>,
    stream: Option<StreamController>,
    /// `true` entre la soumission et le premier AssistantDelta : on attend de
    /// connaître l'ordre réel (outil d'abord ? texte d'abord ?) avant de pousser
    /// l'AssistantCell, pour que les ToolCells s'intercalent naturellement avant.
    pending_assistant_cell: bool,
    active_tool: Option<String>,
    server_rx: mpsc::UnboundedReceiver<ServerEvent>,
    out_tx: mpsc::UnboundedSender<OutboundMsg>,
    running: bool,
    agent_running: bool,
}

impl App {
    pub fn new(
        session_id: String,
        server_rx: mpsc::UnboundedReceiver<ServerEvent>,
        out_tx: mpsc::UnboundedSender<OutboundMsg>,
    ) -> Self {
        Self {
            session_id,
            transcript: TranscriptWidget::new(),
            composer: ComposerWidget::new(),
            overlay_stack: Vec::new(),
            stream: None,
            pending_assistant_cell: false,
            active_tool: None,
            server_rx,
            out_tx,
            running: true,
            agent_running: false,
        }
    }

    pub async fn run(&mut self, terminal: &mut Tui) -> anyhow::Result<()> {
        terminal.draw(|f| self.draw(f))?;

        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        while self.running {
            tokio::select! {
                _ = tick.tick() => {
                    let mut needs_redraw = self.transcript.tick();
                    needs_redraw |= self.tick_stream();
                    if needs_redraw {
                        terminal.draw(|f| self.draw(f))?;
                    }
                }
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) => {
                            self.handle_event(AppEvent::Key(key));
                            terminal.draw(|f| self.draw(f))?;
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            use crossterm::event::MouseEventKind;
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    self.transcript.scroll(-3);
                                    terminal.draw(|f| self.draw(f))?;
                                }
                                MouseEventKind::ScrollDown => {
                                    self.transcript.scroll(3);
                                    terminal.draw(|f| self.draw(f))?;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                Some(server_ev) = self.server_rx.recv() => {
                    self.handle_event(AppEvent::Server(server_ev));
                    terminal.draw(|f| self.draw(f))?;
                }
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, event: AppEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match event {
            AppEvent::Key(key) => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                // Raccourcis globaux (non bloqués par les overlays)
                match key.code {
                    KeyCode::Char('q') if ctrl => {
                        self.running = false;
                        return;
                    }
                    KeyCode::Char('c') if ctrl => {
                        if !self.agent_running {
                            self.running = false;
                        }
                        return;
                    }
                    KeyCode::PageUp => {
                        self.transcript.scroll(-20);
                        return;
                    }
                    KeyCode::PageDown => {
                        self.transcript.scroll(20);
                        return;
                    }
                    // macOS : Option+↑ / Option+↓  (= Alt dans les terminaux)
                    KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                        self.transcript.scroll(-3);
                        return;
                    }
                    KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                        self.transcript.scroll(3);
                        return;
                    }
                    _ => {}
                }

                // L'overlay actif intercepte le clavier en premier.
                if let Some(overlay) = self.overlay_stack.last_mut() {
                    match overlay.handle_key(key) {
                        ViewAction::Consumed => {}
                        ViewAction::Done(value) => {
                            let token = overlay.token().to_owned();
                            self.overlay_stack.pop();
                            if let Some(v) = value {
                                let _ = self.out_tx.send(OutboundMsg::PromptResponse {
                                    token,
                                    value: v,
                                });
                            }
                        }
                    }
                    return;
                }

                // Router vers le composer
                if let Some(output) = self.composer.handle_key(key) {
                    match output {
                        ComposerOutput::Submit(text) => {
                            self.handle_event(AppEvent::Submit(text));
                        }
                        ComposerOutput::Handled => {}
                    }
                }
            }
            AppEvent::Submit(text) => {
                let _ = self.out_tx.send(OutboundMsg::SendMessage(text.clone()));
                self.transcript.push(Box::new(UserCell::new(text)));
                self.stream = Some(StreamController::new());
                self.pending_assistant_cell = true;
                self.agent_running = true;
                self.composer.set_agent_running(true);
            }
            AppEvent::Server(ev) => self.handle_server(ev),
        }
    }

    fn handle_server(&mut self, ev: ServerEvent) {
        match ev {
            ServerEvent::ConversationReady(id) => {
                self.session_id = id;
            }
            ServerEvent::AssistantDelta(delta) => {
                if self.pending_assistant_cell {
                    self.transcript.push(Box::new(AssistantCell::new_streaming()));
                    self.pending_assistant_cell = false;
                }
                if let Some(ref mut stream) = self.stream {
                    stream.push(&delta);
                }
            }
            ServerEvent::AssistantDone => {
                if self.pending_assistant_cell {
                    // L'agent a répondu sans émettre de texte (outil seul sans synthèse).
                    self.pending_assistant_cell = false;
                }
                self.finalize_stream();
                self.stream = None;
                self.agent_running = false;
            }
            ServerEvent::ToolStart { call_id, tool, args } => {
                self.active_tool = Some(tool.clone());
                self.transcript.push(Box::new(ToolCell::new(call_id, tool, args)));
            }
            ServerEvent::ToolResult { call_id, tool: _, result, is_error } => {
                self.active_tool = None;
                if let Some(cell) = self.find_tool_mut(&call_id) {
                    cell.set_result(result, is_error);
                }
            }
            ServerEvent::ApprovalRequest { token, label, options } => {
                self.transcript.push(Box::new(ApprovalCell::new(
                    token.clone(),
                    label.clone(),
                    options.clone(),
                )));
                self.overlay_stack.push(Box::new(ApprovalOverlay::new(
                    token, label, options,
                )));
            }
            ServerEvent::AgentDone | ServerEvent::AgentCancelled(_) => {
                self.pending_assistant_cell = false;
                self.agent_running = false;
                self.composer.set_agent_running(false);
            }
            ServerEvent::Error(msg) => {
                // Phase 6a: afficher dans le transcript
                let _ = msg;
                self.agent_running = false;
            }
            ServerEvent::ConnectionClosed => {
                self.running = false;
            }
            ServerEvent::ToolProgress { .. } => {}
        }
    }

    /// Émet N lignes du StreamController vers la dernière AssistantCell du transcript.
    /// Nécessaire car des ToolCells peuvent être intercalées après la cellule streaming.
    fn tick_stream(&mut self) -> bool {
        if self.stream.is_none() {
            return false;
        }
        let stream = self.stream.as_mut().expect("checked above");
        for cell in self.transcript.cells_mut().rev() {
            if let Some(assistant) = cell.as_any_mut().downcast_mut::<AssistantCell>() {
                return stream.on_tick(assistant);
            }
        }
        false
    }

    /// Finalise le StreamController sur la dernière AssistantCell du transcript.
    fn finalize_stream(&mut self) {
        if self.stream.is_none() {
            return;
        }
        let stream = self.stream.as_mut().expect("checked above");
        for cell in self.transcript.cells_mut().rev() {
            if let Some(assistant) = cell.as_any_mut().downcast_mut::<AssistantCell>() {
                stream.finalize(assistant);
                return;
            }
        }
    }

    fn find_tool_mut(&mut self, call_id: &str) -> Option<&mut ToolCell> {
        for cell in self.transcript.cells_mut().rev() {
            if let Some(tc) = cell.as_any_mut().downcast_mut::<ToolCell>() {
                if tc.call_id == call_id {
                    return Some(tc);
                }
            }
        }
        None
    }

    fn draw(&self, frame: &mut Frame) {
        let composer_height = self.composer.desired_height();
        let status_height: u16 = 1;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(composer_height),
                Constraint::Length(status_height),
            ])
            .split(frame.area());

        self.transcript.render(frame, chunks[0]);
        self.composer.render(frame, chunks[1]);

        let status = StatusLine {
            session_id: self.session_id.clone(),
            agent_running: self.agent_running,
            active_tool: self.active_tool.clone(),
        };
        frame.render_widget(status.render(), chunks[2]);

        // Rendu de l'overlay actif par-dessus tout.
        if let Some(overlay) = self.overlay_stack.last() {
            overlay.render(frame, frame.area());
        }
    }
}

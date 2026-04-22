use crossterm::event::KeyEvent;

use crate::client::ServerEvent;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Server(ServerEvent),
    Submit(String),
}

mod connection;
mod credential;
mod event;

pub use connection::{ClientError, OzzieClient, OpenConversationOpts, PairingRequest, PairingStatus};

pub use credential::{Credential, CredentialError, CredentialStore, FileCredentialStore, MemoryCredentialStore};
pub use event::ClientEvent;
pub use ozzie_protocol::{self as protocol, EventKind, Frame, PromptRequestPayload, PromptResponseParams};

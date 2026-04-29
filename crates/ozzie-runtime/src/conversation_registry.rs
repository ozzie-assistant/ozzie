use std::sync::Arc;
use std::sync::RwLock;

use chrono::Utc;
use dashmap::DashMap;

use crate::conversation_runtime::ConversationRuntime;
use ozzie_core::domain::{
    Conversation, ConversationError, ConversationManager, ConversationStatus, ConversationStore,
    ConversationSummary,
};
use ozzie_core::events::{Event, EventBus, EventPayload, EventSource};

/// Runtime registry for conversations.
///
/// Owns the per-conversation `ConversationRuntime` instances (cancel tokens,
/// pending message buffers, active flags) and tracks the single
/// `active_conversation_id` that represents the user's current attention.
///
/// `touch()` implements last-touched semantics: any interaction with a
/// conversation promotes it to active. Archive demotes: if the archived
/// conversation was active, active falls back to `None` (caller decides
/// what to do next).
pub struct ConversationRegistry {
    store: Arc<dyn ConversationStore>,
    runtimes: DashMap<String, Arc<ConversationRuntime>>,
    active_id: RwLock<Option<String>>,
    bus: Option<Arc<dyn EventBus>>,
    /// Binds a connector coordinate (e.g. "discord:server:channel:user") to
    /// the conversation it feeds into. Persisted implicitly via each
    /// conversation's `metadata.connector_coord` field.
    bindings: DashMap<String, String>,
}

impl ConversationRegistry {
    pub fn new(store: Arc<dyn ConversationStore>) -> Self {
        Self {
            store,
            runtimes: DashMap::new(),
            active_id: RwLock::new(None),
            bus: None,
            bindings: DashMap::new(),
        }
    }

    /// Attaches an event bus so registry mutations emit observable events.
    pub fn with_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Promotes the most recently updated non-archived conversation as active,
    /// and rebuilds connector bindings from persisted metadata.
    ///
    /// Called once at boot so the user's focus and connector routing survive
    /// gateway restarts. Silently skips if the store is empty or errors.
    pub async fn bootstrap_active_from_store(&self) {
        let Ok(conversations) = self.store.list().await else {
            return;
        };

        // Rebuild bindings from persisted metadata.
        for c in &conversations {
            if let Some(coord) = c.metadata.get("connector_coord") {
                self.bindings.insert(coord.clone(), c.id.clone());
            }
        }

        let mut candidates: Vec<_> = conversations
            .into_iter()
            .filter(|c| matches!(c.status, ConversationStatus::Active))
            .collect();
        candidates.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
        if let Some(most_recent) = candidates.first() {
            let mut guard = self.active_id.write().unwrap_or_else(|e| e.into_inner());
            *guard = Some(most_recent.id.clone());
        }
    }

    /// Resolves (or creates) the conversation bound to a connector coordinate.
    ///
    /// On first contact from a coordinate, creates a new conversation, binds it,
    /// and stores `connector_coord` in its metadata for later rebinding.
    /// The resolved conversation is NOT touched — the caller decides whether
    /// this coordinate should steal the active focus.
    ///
    /// If a binding already exists but points to an archived conversation,
    /// it is replaced with a fresh one. This is what powers connector-side
    /// commands like Discord's `/new`: archive the conversation, the next
    /// message from the same coordinate spawns a fresh thread.
    pub async fn resolve_or_create_for_coord(
        &self,
        coord_key: &str,
        title: Option<String>,
    ) -> Result<String, ConversationError> {
        if let Some(existing) = self.bindings.get(coord_key) {
            let id = existing.clone();
            drop(existing);
            // Verify the bound conversation is still Active; otherwise rebind.
            match self.store.get(&id).await? {
                Some(c) if matches!(c.status, ConversationStatus::Active) => return Ok(id),
                _ => {
                    self.bindings.remove(coord_key);
                }
            }
        }

        let id = self.generate_id();
        let mut conversation = Conversation::new(&id);
        conversation.title = title.clone();
        conversation
            .metadata
            .insert("connector_coord".to_string(), coord_key.to_string());
        self.store.create(&conversation).await?;
        self.bindings.insert(coord_key.to_string(), id.clone());

        self.publish(
            EventPayload::ConversationCreated {
                conversation_id: id.clone(),
                title,
            },
            &id,
        );
        Ok(id)
    }

    /// Returns the conversation id currently bound to a coordinate, if any.
    pub fn coord_binding(&self, coord_key: &str) -> Option<String> {
        self.bindings.get(coord_key).map(|v| v.clone())
    }

    /// Emits `ConversationUnread` — a hint for connectors to surface a badge
    /// without polluting the active conversation's history.
    pub fn mark_unread(&self, conversation_id: &str, last_event_kind: Option<&str>) {
        self.publish(
            EventPayload::ConversationUnread {
                conversation_id: conversation_id.to_string(),
                last_event_kind: last_event_kind.map(|s| s.to_string()),
            },
            conversation_id,
        );
    }

    fn publish(&self, payload: EventPayload, conversation_id: &str) {
        if let Some(bus) = &self.bus {
            bus.publish(Event::with_session(EventSource::Agent, payload, conversation_id));
        }
    }

    /// Returns a handle to the persistence store.
    pub fn store(&self) -> &Arc<dyn ConversationStore> {
        &self.store
    }

    /// Returns (or creates) the runtime for a conversation id.
    ///
    /// Does not touch active state — that is a separate concern.
    pub fn get_or_create_runtime(&self, id: &str) -> Arc<ConversationRuntime> {
        self.runtimes
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(ConversationRuntime::new()))
            .clone()
    }

    /// Returns the runtime for a conversation id if one exists.
    pub fn get_runtime(&self, id: &str) -> Option<Arc<ConversationRuntime>> {
        self.runtimes.get(id).map(|r| r.clone())
    }

    /// Returns the currently active conversation id, if any.
    pub fn active(&self) -> Option<String> {
        self.active_id.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Sets the active conversation explicitly. Returns the previous active id.
    ///
    /// Does not verify the conversation exists — caller is responsible.
    /// Emits `ConversationSwitched` when the active id actually changes.
    pub fn set_active(&self, id: &str) -> Option<String> {
        let previous = {
            let mut guard = self.active_id.write().unwrap_or_else(|e| e.into_inner());
            let previous = guard.clone();
            *guard = Some(id.to_string());
            previous
        };
        if previous.as_deref() != Some(id) {
            self.publish(
                EventPayload::ConversationSwitched {
                    from: previous.clone(),
                    to: id.to_string(),
                },
                id,
            );
        }
        previous
    }

    /// Clears the active conversation pointer.
    pub fn clear_active(&self) -> Option<String> {
        let mut guard = self.active_id.write().unwrap_or_else(|e| e.into_inner());
        guard.take()
    }

    /// Marks a conversation as last-touched (becomes active).
    ///
    /// Returns the previous active id if it changed, `None` otherwise.
    /// Emits `ConversationSwitched` when the active id actually changes.
    pub fn touch(&self, id: &str) -> Option<String> {
        let previous = {
            let mut guard = self.active_id.write().unwrap_or_else(|e| e.into_inner());
            match guard.as_deref() {
                Some(cur) if cur == id => return None,
                _ => {
                    let previous = guard.clone();
                    *guard = Some(id.to_string());
                    previous
                }
            }
        };
        self.publish(
            EventPayload::ConversationSwitched {
                from: previous.clone(),
                to: id.to_string(),
            },
            id,
        );
        previous
    }

    /// Creates a new conversation in the store and sets it as active.
    ///
    /// Emits `ConversationCreated` and `ConversationSwitched`.
    /// Returns the new conversation id.
    pub async fn create_conversation(
        &self,
        title: Option<String>,
    ) -> Result<String, ConversationError> {
        let id = self.generate_id();
        let mut conversation = Conversation::new(&id);
        conversation.title = title.clone();
        self.store.create(&conversation).await?;
        self.publish(
            EventPayload::ConversationCreated {
                conversation_id: id.clone(),
                title,
            },
            &id,
        );
        self.set_active(&id);
        Ok(id)
    }

    /// Lists all conversations from the store, marking the active one.
    pub async fn list(&self) -> Result<Vec<ConversationSummary>, ConversationError> {
        let conversations = self.store.list().await?;
        let active = self.active();
        let mut summaries: Vec<ConversationSummary> = conversations
            .into_iter()
            .map(|c| ConversationSummary {
                is_active: active.as_deref() == Some(c.id.as_str()),
                id: c.id,
                title: c.title,
                status: c.status,
                message_count: c.message_count,
                updated_at: c.updated_at,
            })
            .collect();
        summaries.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(summaries)
    }

    /// Archives a conversation (freeze + hide, history preserved).
    ///
    /// If the archived conversation was the active one, active is cleared.
    /// Caller decides what to do next (promote another, create new, etc.).
    /// Emits `ConversationArchived` (and `ConversationSwitched` to `None` when
    /// the archived conversation was active).
    pub async fn archive(&self, id: &str) -> Result<(), ConversationError> {
        if let Some(runtime) = self.get_runtime(id)
            && runtime.is_active()
        {
            return Err(ConversationError::Other(format!(
                "cannot archive conversation {id}: a ReactLoop is currently running"
            )));
        }
        self.store.archive(id).await?;
        let was_active = {
            let mut guard = self.active_id.write().unwrap_or_else(|e| e.into_inner());
            if guard.as_deref() == Some(id) {
                *guard = None;
                true
            } else {
                false
            }
        };
        self.publish(
            EventPayload::ConversationArchived {
                conversation_id: id.to_string(),
            },
            id,
        );
        if was_active {
            self.publish(
                EventPayload::ConversationSwitched {
                    from: Some(id.to_string()),
                    to: String::new(),
                },
                id,
            );
        }
        Ok(())
    }

    /// Unarchives a conversation (brings it back to Active status).
    ///
    /// Does not set it as active — caller can `set_active` or `touch` separately.
    pub async fn unarchive(&self, id: &str) -> Result<(), ConversationError> {
        let mut conversation = self
            .store
            .get(id)
            .await?
            .ok_or_else(|| ConversationError::NotFound(id.to_string()))?;
        conversation.status = ConversationStatus::Active;
        conversation.updated_at = Utc::now();
        self.store.update(&conversation).await
    }

    fn generate_id(&self) -> String {
        let runtimes = &self.runtimes;
        ozzie_utils::names::generate_id("sess", |candidate| runtimes.contains_key(candidate))
    }
}

#[async_trait::async_trait]
impl ConversationManager for ConversationRegistry {
    fn active(&self) -> Option<String> {
        ConversationRegistry::active(self)
    }

    fn set_active(&self, id: &str) -> Option<String> {
        ConversationRegistry::set_active(self, id)
    }

    async fn create(&self, title: Option<String>) -> Result<String, ConversationError> {
        self.create_conversation(title).await
    }

    async fn list(&self) -> Result<Vec<ConversationSummary>, ConversationError> {
        ConversationRegistry::list(self).await
    }

    async fn archive(&self, id: &str) -> Result<(), ConversationError> {
        ConversationRegistry::archive(self, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::InMemoryConversationStore;

    fn new_registry() -> ConversationRegistry {
        ConversationRegistry::new(Arc::new(InMemoryConversationStore::new()))
    }

    #[test]
    fn new_registry_has_no_active() {
        let reg = new_registry();
        assert!(reg.active().is_none());
    }

    #[test]
    fn get_or_create_runtime_is_idempotent() {
        let reg = new_registry();
        let a = reg.get_or_create_runtime("conv_a");
        let b = reg.get_or_create_runtime("conv_a");
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn touch_sets_active_and_returns_previous() {
        let reg = new_registry();
        assert_eq!(reg.touch("a"), None);
        assert_eq!(reg.active().as_deref(), Some("a"));
        assert_eq!(reg.touch("b").as_deref(), Some("a"));
        assert_eq!(reg.active().as_deref(), Some("b"));
    }

    #[test]
    fn touch_same_id_is_noop() {
        let reg = new_registry();
        reg.touch("a");
        let ret = reg.touch("a");
        assert!(ret.is_none());
        assert_eq!(reg.active().as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn create_conversation_sets_active() {
        let reg = new_registry();
        let id = reg.create_conversation(Some("first".into())).await.unwrap();
        assert_eq!(reg.active().as_deref(), Some(id.as_str()));
        let listed = reg.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_active);
        assert_eq!(listed[0].title.as_deref(), Some("first"));
    }

    #[tokio::test]
    async fn archive_clears_active_when_matching() {
        let reg = new_registry();
        let id = reg.create_conversation(None).await.unwrap();
        reg.archive(&id).await.unwrap();
        assert!(reg.active().is_none());
    }

    #[tokio::test]
    async fn archive_preserves_other_active() {
        let reg = new_registry();
        let a = reg.create_conversation(None).await.unwrap();
        let b = reg.create_conversation(None).await.unwrap();
        assert_eq!(reg.active().as_deref(), Some(b.as_str()));

        reg.archive(&a).await.unwrap();
        assert_eq!(reg.active().as_deref(), Some(b.as_str()));
    }

    #[tokio::test]
    async fn archive_refuses_while_runtime_active() {
        let reg = new_registry();
        let id = reg.create_conversation(None).await.unwrap();
        let rt = reg.get_or_create_runtime(&id);
        rt.set_active(true);

        let err = reg.archive(&id).await.unwrap_err();
        assert!(matches!(err, ConversationError::Other(_)));
    }

    #[tokio::test]
    async fn resolve_or_create_for_coord_is_idempotent() {
        let reg = new_registry();
        let first = reg
            .resolve_or_create_for_coord("discord:g1:c1:u1", Some("test".into()))
            .await
            .unwrap();
        let second = reg
            .resolve_or_create_for_coord("discord:g1:c1:u1", None)
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(reg.coord_binding("discord:g1:c1:u1").as_deref(), Some(first.as_str()));
    }

    #[tokio::test]
    async fn resolve_or_create_for_coord_does_not_touch_active() {
        let reg = new_registry();
        // Pre-existing active conversation.
        let a = reg.create_conversation(Some("main".into())).await.unwrap();
        assert_eq!(reg.active().as_deref(), Some(a.as_str()));

        // Incoming coord resolves to its own conv, active pointer unchanged.
        let bound = reg
            .resolve_or_create_for_coord("discord:g:c:stranger", None)
            .await
            .unwrap();
        assert_ne!(bound, a);
        assert_eq!(reg.active().as_deref(), Some(a.as_str()));
    }

    #[tokio::test]
    async fn resolve_for_coord_rebinds_when_target_archived() {
        let reg = new_registry();
        let id1 = reg
            .resolve_or_create_for_coord("discord:g:c:u", None)
            .await
            .unwrap();
        // Archive the conversation behind the binding.
        reg.archive(&id1).await.unwrap();

        // Next resolution should create a fresh one.
        let id2 = reg
            .resolve_or_create_for_coord("discord:g:c:u", None)
            .await
            .unwrap();
        assert_ne!(id1, id2);
        assert_eq!(reg.coord_binding("discord:g:c:u").as_deref(), Some(id2.as_str()));
    }

    #[tokio::test]
    async fn bootstrap_rebuilds_bindings_from_metadata() {
        let store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        // Seed a conversation with a coord in metadata.
        let mut c = Conversation::new("conv_seed");
        c.metadata
            .insert("connector_coord".into(), "discord:g:c:u".into());
        store.create(&c).await.unwrap();

        let reg = ConversationRegistry::new(store.clone());
        reg.bootstrap_active_from_store().await;
        assert_eq!(
            reg.coord_binding("discord:g:c:u").as_deref(),
            Some("conv_seed"),
        );
    }

    #[tokio::test]
    async fn unarchive_restores_active_status() {
        let reg = new_registry();
        let id = reg.create_conversation(None).await.unwrap();
        reg.archive(&id).await.unwrap();

        reg.unarchive(&id).await.unwrap();
        let got = reg.store.get(&id).await.unwrap().unwrap();
        assert_eq!(got.status, ConversationStatus::Active);
    }

    #[tokio::test]
    async fn list_sorts_by_updated_desc() {
        let reg = new_registry();
        let a = reg.create_conversation(Some("A".into())).await.unwrap();
        // Force a tick so updated_at differs
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let b = reg.create_conversation(Some("B".into())).await.unwrap();

        let listed = reg.list().await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, b);
        assert_eq!(listed[1].id, a);
    }

    #[test]
    fn clear_active_returns_previous() {
        let reg = new_registry();
        reg.touch("x");
        let prev = reg.clear_active();
        assert_eq!(prev.as_deref(), Some("x"));
        assert!(reg.active().is_none());
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use ozzie_core::domain::{DreamRecord, DreamStats, MemorySchema, Message, PageStore};
use ozzie_core::events::{Event, EventBus, EventPayload, EventSource};
use ozzie_llm::Provider;
use ozzie_memory::{ImportanceLevel, MemoryEntry, MemoryType, Store};

use crate::session::{Session, SessionStatus};
use crate::SessionStore;

use super::classifier;
use super::record_store::DreamRecordStore;
use super::synthesizer::Synthesizer;
use super::workspace_record::{WorkspaceRecord, WorkspaceRecordStore};
use super::workspace_scanner;

const DEFAULT_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60); // 12 hours
const DEFAULT_ACTIVE_MIN_AGE: Duration = Duration::from_secs(2 * 60 * 60); // 2 hours
const MIN_MESSAGES_TO_PROCESS: usize = 4;

/// Runs periodic dream consolidation — extracts lasting knowledge from conversations
/// and project workspaces.
pub struct DreamRunner {
    sessions: Arc<dyn SessionStore>,
    memory_store: Arc<dyn Store>,
    page_store: Option<Arc<dyn PageStore>>,
    provider: Arc<dyn Provider>,
    bus: Arc<dyn EventBus>,
    ozzie_path: PathBuf,
    interval: Duration,
    active_min_age: Duration,
    project_registry: Option<Arc<ozzie_core::project::ProjectRegistry>>,
    cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl DreamRunner {
    pub fn new(
        sessions: Arc<dyn SessionStore>,
        memory_store: Arc<dyn Store>,
        provider: Arc<dyn Provider>,
        ozzie_path: &Path,
        bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            sessions,
            memory_store,
            page_store: None,
            provider,
            bus,
            ozzie_path: ozzie_path.to_path_buf(),
            interval: DEFAULT_INTERVAL,
            active_min_age: DEFAULT_ACTIVE_MIN_AGE,
            project_registry: None,
            cancel: Mutex::new(None),
        }
    }

    /// Enables wiki page synthesis during dream runs.
    pub fn with_page_store(mut self, page_store: Arc<dyn PageStore>) -> Self {
        self.page_store = Some(page_store);
        self
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Enables workspace scanning for project consolidation.
    pub fn with_project_registry(
        mut self,
        registry: Arc<ozzie_core::project::ProjectRegistry>,
    ) -> Self {
        self.project_registry = Some(registry);
        self
    }

    /// Starts the background dream loop.
    pub async fn start(self: &Arc<Self>) {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        *self.cancel.lock().await = Some(tx);

        let runner = Arc::clone(self);
        let interval = self.interval;

        tokio::spawn(async move {
            // Run once immediately on startup
            let stats = runner.run_once().await;
            info!(%stats, "dream initial run complete");

            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // skip first immediate tick

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let stats = runner.run_once().await;
                        info!(%stats, "dream run complete");
                    }
                    _ = &mut rx => {
                        debug!("dream runner stopped");
                        break;
                    }
                }
            }
        });
    }

    /// Stops the background loop.
    pub async fn stop(&self) {
        if let Some(tx) = self.cancel.lock().await.take() {
            let _ = tx.send(());
        }
    }

    /// Runs a single consolidation pass across all eligible sessions.
    pub async fn run_once(&self) -> DreamStats {
        let mut stats = DreamStats::default();
        let record_store = DreamRecordStore::new(&self.ozzie_path);
        let mut records = record_store.load_all();

        let sessions = match self.sessions.list().await {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "dream: failed to list sessions");
                return stats;
            }
        };

        let now = Utc::now();

        for session in &sessions {
            if !is_eligible(session, now, self.active_min_age) {
                stats.sessions_skipped += 1;
                continue;
            }

            let previous = records.get(&session.id);

            // Skip if already fully consolidated
            if let Some(rec) = previous
                && rec.consolidated_up_to >= session.message_count
            {
                stats.sessions_skipped += 1;
                continue;
            }

            match self.process_session(session, previous).await {
                Ok(Some(record)) => {
                    stats.profile_entries_added += record.profile_entries.len()
                        - previous.map_or(0, |r| r.profile_entries.len());
                    stats.memories_created += record.memory_ids.len()
                        - previous.map_or(0, |r| r.memory_ids.len());
                    stats.sessions_processed += 1;

                    records.insert(session.id.clone(), record.clone());
                    if let Err(e) = record_store.save(&record) {
                        error!(
                            session_id = %session.id,
                            error = %e,
                            "dream: failed to save record"
                        );
                    }
                }
                Ok(None) => {
                    stats.sessions_skipped += 1;
                }
                Err(e) => {
                    warn!(
                        session_id = %session.id,
                        error = %e,
                        "dream: failed to process session"
                    );
                    stats.sessions_errored += 1;
                }
            }
        }

        // Workspace scanning — runs after session classification.
        if let Some(ref project_registry) = self.project_registry {
            let ws_record_store = WorkspaceRecordStore::new(&self.ozzie_path);
            let ws_records = ws_record_store.load_all();
            let now = Utc::now();

            for manifest in project_registry.all() {
                // Only scan projects with memory config
                let Some(ref mem_config) = manifest.memory else {
                    continue;
                };

                // Check scan_cron — if set, only scan when the cron matches
                if let Some(ref cron_spec) = mem_config.scan_cron {
                    match crate::scheduler::CronExpr::parse(cron_spec) {
                        Ok(cron) => {
                            if !cron.matches(&now) {
                                continue;
                            }
                        }
                        Err(e) => {
                            warn!(
                                project = %manifest.name,
                                cron = %cron_spec,
                                error = %e,
                                "invalid scan_cron, skipping"
                            );
                            continue;
                        }
                    }
                }

                let previous = ws_records.get(&manifest.name);

                match workspace_scanner::scan_workspace(
                    self.provider.as_ref(),
                    &manifest,
                    previous,
                )
                .await
                {
                    Ok(Some(result)) => {
                        let mut record = previous.cloned().unwrap_or_else(|| WorkspaceRecord {
                            project_name: manifest.name.clone(),
                            last_commit: String::new(),
                            memory_ids: Vec::new(),
                            updated_at: Utc::now(),
                        });

                        // Save extracted memories
                        for mem_entry in &result.extraction.memory {
                            let mem_type =
                                mem_entry.memory_type.parse().unwrap_or(MemoryType::Context);
                            let now = Utc::now();

                            let mut entry = MemoryEntry {
                                id: String::new(),
                                title: mem_entry.title.clone(),
                                source: format!("workspace:{}", manifest.name),
                                memory_type: mem_type,
                                tags: mem_entry.tags.clone(),
                                created_at: now,
                                updated_at: now,
                                last_used_at: now,
                                confidence: 0.8,
                                importance: ImportanceLevel::Normal,
                                embedding_model: String::new(),
                                indexed_at: None,
                                merged_into: None,
                            };

                            match self
                                .memory_store
                                .create(&mut entry, &mem_entry.content)
                                .await
                            {
                                Ok(()) => {
                                    record.memory_ids.push(entry.id.clone());
                                    stats.memories_created += 1;
                                    debug!(
                                        id = %entry.id,
                                        project = %manifest.name,
                                        title = %entry.title,
                                        "workspace: memory created"
                                    );
                                }
                                Err(e) => {
                                    warn!(
                                        project = %manifest.name,
                                        title = %mem_entry.title,
                                        error = %e,
                                        "workspace: failed to create memory"
                                    );
                                }
                            }
                        }

                        // Save profile entries
                        if !result.extraction.profile.is_empty() {
                            if let Err(e) =
                                self.save_profile_entries(&result.extraction.profile)
                            {
                                error!(error = %e, "workspace: failed to save profile entries");
                            } else {
                                stats.profile_entries_added +=
                                    result.extraction.profile.len();
                            }
                        }

                        record.last_commit = result.head_commit;
                        record.updated_at = Utc::now();

                        if let Err(e) = ws_record_store.save(&record) {
                            error!(
                                project = %manifest.name,
                                error = %e,
                                "workspace: failed to save record"
                            );
                        }
                    }
                    Ok(None) => {
                        // Nothing changed
                    }
                    Err(e) => {
                        warn!(
                            project = %manifest.name,
                            error = %e,
                            "workspace: scan failed"
                        );
                    }
                }
            }
        }

        // Wiki page synthesis — runs after all sessions and workspaces are classified.
        if let Some(ref page_store) = self.page_store {
            let schema = MemorySchema::load(&self.ozzie_path);
            let synthesizer = Synthesizer::new(
                self.memory_store.clone(),
                page_store.clone(),
                self.provider.clone(),
            )
            .with_schema(schema.clone());
            match synthesizer.synthesize().await {
                Ok(syn_stats) => {
                    stats.pages_created = syn_stats.pages_created;
                    stats.pages_updated = syn_stats.pages_updated;
                }
                Err(e) => {
                    warn!(error = %e, "dream: wiki synthesis failed");
                }
            }

            // Generate index and run lint
            let pages_dir = self.ozzie_path.join("memory").join("pages");
            if let Err(e) = super::index_generator::generate_index(
                &pages_dir,
                page_store.as_ref(),
                self.memory_store.as_ref(),
            )
            .await
            {
                warn!(error = %e, "dream: index generation failed");
            }

            let warnings = super::lint::lint(page_store.as_ref(), self.memory_store.as_ref(), Some(&schema)).await;
            if !warnings.is_empty() {
                info!(count = warnings.len(), "dream: lint warnings detected");
            }
        }

        // Emit event for observability
        self.bus.publish(Event::new(
            EventSource::Agent,
            EventPayload::DreamCompleted {
                sessions_processed: stats.sessions_processed,
                sessions_errored: stats.sessions_errored,
                profile_entries_added: stats.profile_entries_added,
                memories_created: stats.memories_created,
            },
        ));

        stats
    }

    /// Processes a single session. Returns `None` if nothing to do.
    async fn process_session(
        &self,
        session: &Session,
        previous: Option<&DreamRecord>,
    ) -> anyhow::Result<Option<DreamRecord>> {
        let messages = self.sessions.load_messages(&session.id).await?;
        let start_idx = previous.map_or(0, |r| r.consolidated_up_to);

        if start_idx >= messages.len() {
            return Ok(None);
        }

        let new_messages = &messages[start_idx..];

        // Skip if too few substantive messages
        let substantive: Vec<&Message> = new_messages
            .iter()
            .filter(|m| {
                !m.content.trim().is_empty()
                    && (m.role == "user" || m.role == "assistant")
            })
            .collect();

        if substantive.len() < MIN_MESSAGES_TO_PROCESS {
            return Ok(None);
        }

        debug!(
            session_id = %session.id,
            new_messages = new_messages.len(),
            from_idx = start_idx,
            "dream: classifying session"
        );

        let extraction =
            classifier::classify_session(self.provider.as_ref(), new_messages, previous)
                .await?;

        // Build updated record
        let mut record = previous.cloned().unwrap_or_else(|| DreamRecord {
            session_id: session.id.clone(),
            consolidated_up_to: 0,
            profile_entries: Vec::new(),
            memory_ids: Vec::new(),
            updated_at: Utc::now(),
        });

        // Save profile entries
        if !extraction.profile.is_empty() {
            if let Err(e) = self.save_profile_entries(&extraction.profile) {
                error!(error = %e, "dream: failed to save profile entries");
                // Continue — still save memories
            } else {
                record
                    .profile_entries
                    .extend(extraction.profile.iter().cloned());
            }
        }

        // Save memory entries
        for mem_entry in &extraction.memory {
            let mem_type = mem_entry.memory_type.parse().unwrap_or(MemoryType::Context);
            let now = Utc::now();

            let mut entry = MemoryEntry {
                id: String::new(), // generated by store
                title: mem_entry.title.clone(),
                source: "dream".to_string(),
                memory_type: mem_type,
                tags: mem_entry.tags.clone(),
                created_at: now,
                updated_at: now,
                last_used_at: now,
                confidence: 0.8,
                importance: ImportanceLevel::Normal,
                embedding_model: String::new(),
                indexed_at: None,
                merged_into: None,
            };

            match self.memory_store.create(&mut entry, &mem_entry.content).await {
                Ok(()) => {
                    record.memory_ids.push(entry.id.clone());
                    debug!(
                        id = %entry.id,
                        title = %entry.title,
                        "dream: memory created"
                    );
                }
                Err(e) => {
                    warn!(
                        title = %mem_entry.title,
                        error = %e,
                        "dream: failed to create memory"
                    );
                }
            }
        }

        record.consolidated_up_to = messages.len();
        record.updated_at = Utc::now();

        Ok(Some(record))
    }

    fn save_profile_entries(&self, entries: &[String]) -> anyhow::Result<()> {
        let mut profile = ozzie_core::profile::load(&self.ozzie_path)
            .map_err(|e| anyhow::anyhow!(e))?
            .ok_or_else(|| anyhow::anyhow!("no profile found"))?;

        for entry in entries {
            profile.add_observation(entry.clone());
        }

        ozzie_core::profile::save(&self.ozzie_path, &profile)
            .map_err(|e| anyhow::anyhow!(e))
    }
}

fn is_eligible(session: &Session, now: chrono::DateTime<Utc>, active_min_age: Duration) -> bool {
    match session.status {
        SessionStatus::Closed => true,
        SessionStatus::Active => {
            let age = now - session.updated_at;
            age.to_std().unwrap_or(Duration::ZERO) >= active_min_age
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_session_is_eligible() {
        let session = Session {
            id: "s1".to_string(),
            status: SessionStatus::Closed,
            updated_at: Utc::now(),
            ..test_session()
        };
        assert!(is_eligible(&session, Utc::now(), DEFAULT_ACTIVE_MIN_AGE));
    }

    #[test]
    fn recent_active_session_not_eligible() {
        let session = Session {
            id: "s2".to_string(),
            status: SessionStatus::Active,
            updated_at: Utc::now(),
            ..test_session()
        };
        assert!(!is_eligible(&session, Utc::now(), DEFAULT_ACTIVE_MIN_AGE));
    }

    #[test]
    fn old_active_session_is_eligible() {
        let session = Session {
            id: "s3".to_string(),
            status: SessionStatus::Active,
            updated_at: Utc::now() - chrono::Duration::hours(3),
            ..test_session()
        };
        assert!(is_eligible(&session, Utc::now(), DEFAULT_ACTIVE_MIN_AGE));
    }

    fn test_session() -> Session {
        Session {
            id: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            status: SessionStatus::Active,
            model: None,
            root_dir: None,
            summary: None,
            summary_up_to: 0,
            language: None,
            title: None,
            message_count: 0,
            token_usage: Default::default(),
            approved_tools: Vec::new(),
            metadata: Default::default(),
            policy_name: None,
            project_id: None,
        }
    }
}

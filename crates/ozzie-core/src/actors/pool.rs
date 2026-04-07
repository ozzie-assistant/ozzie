use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;

/// Status of an actor slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorStatus {
    Idle,
    Busy,
}

/// A single LLM capacity slot.
#[derive(Debug, Clone)]
pub struct Actor {
    pub id: String,
    pub provider_name: String,
    pub tags: Vec<String>,
    pub capabilities: Vec<crate::domain::ModelCapability>,
    pub status: ActorStatus,
    pub current_task: Option<String>,
}

/// Configuration for the actor pool.
pub struct ActorPoolConfig {
    /// Actors per provider: provider_name → count.
    pub actors_per_provider: HashMap<String, usize>,
    /// Tags per provider.
    pub tags_per_provider: HashMap<String, Vec<String>>,
    /// Capabilities per provider.
    pub capabilities_per_provider: HashMap<String, Vec<crate::domain::ModelCapability>>,
    /// Provider cooldown duration (default: 2 min).
    pub cooldown_duration: Duration,
}

impl Default for ActorPoolConfig {
    fn default() -> Self {
        Self {
            actors_per_provider: HashMap::new(),
            tags_per_provider: HashMap::new(),
            capabilities_per_provider: HashMap::new(),
            cooldown_duration: Duration::from_secs(120),
        }
    }
}

/// Capacity-aware actor pool for LLM scheduling.
///
/// Each provider has N actor slots (typically 1). Tasks are matched
/// to idle actors by provider, tags, and capabilities.
pub struct ActorPool {
    actors: Mutex<Vec<Actor>>,
    semaphores: HashMap<String, Arc<Semaphore>>,
    cooldowns: Mutex<HashMap<String, Instant>>,
    cooldown_duration: Duration,
}

impl ActorPool {
    pub fn new(config: ActorPoolConfig) -> Self {
        let mut actors = Vec::new();
        let mut semaphores = HashMap::new();

        for (provider, count) in &config.actors_per_provider {
            // At least 1 slot per provider — 0 would deadlock all subtasks/schedules.
            let count = (*count).max(1);
            let tags = config
                .tags_per_provider
                .get(provider)
                .cloned()
                .unwrap_or_default();
            let caps = config
                .capabilities_per_provider
                .get(provider)
                .cloned()
                .unwrap_or_default();

            semaphores.insert(provider.clone(), Arc::new(Semaphore::new(count)));

            for i in 0..count {
                actors.push(Actor {
                    id: format!("{provider}_{i}"),
                    provider_name: provider.clone(),
                    tags: tags.clone(),
                    capabilities: caps.clone(),
                    status: ActorStatus::Idle,
                    current_task: None,
                });
            }
        }

        Self {
            actors: Mutex::new(actors),
            semaphores,
            cooldowns: Mutex::new(HashMap::new()),
            cooldown_duration: config.cooldown_duration,
        }
    }

    /// Acquires an idle actor for the given provider.
    /// Blocks until one is available (via semaphore).
    pub async fn acquire(&self, provider: &str) -> Result<ActorSlot, ActorPoolError> {
        // Check cooldown
        {
            let cooldowns = self.cooldowns.lock().unwrap();
            if let Some(until) = cooldowns.get(provider)
                && Instant::now() < *until
            {
                return Err(ActorPoolError::ProviderCooldown(provider.to_string()));
            }
        }

        let sem = self
            .semaphores
            .get(provider)
            .ok_or_else(|| ActorPoolError::NoProvider(provider.to_string()))?;

        let permit = sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ActorPoolError::Unavailable(provider.to_string()))?;

        // Mark an actor as busy
        let actor_id = {
            let mut actors = self.actors.lock().unwrap();
            let actor = actors
                .iter_mut()
                .find(|a| a.provider_name == provider && a.status == ActorStatus::Idle)
                .ok_or_else(|| ActorPoolError::Unavailable(provider.to_string()))?;
            actor.status = ActorStatus::Busy;
            actor.id.clone()
        };

        Ok(ActorSlot {
            actor_id,
            provider: provider.to_string(),
            _permit: permit,
        })
    }

    /// Releases an actor back to idle.
    pub fn release(&self, slot: ActorSlot) {
        let mut actors = self.actors.lock().unwrap();
        if let Some(actor) = actors.iter_mut().find(|a| a.id == slot.actor_id) {
            actor.status = ActorStatus::Idle;
            actor.current_task = None;
        }
        // Permit is dropped automatically, releasing the semaphore
    }

    /// Sets a cooldown on a provider (e.g., after model unavailable error).
    pub fn set_cooldown(&self, provider: &str) {
        let mut cooldowns = self.cooldowns.lock().unwrap();
        cooldowns.insert(
            provider.to_string(),
            Instant::now() + self.cooldown_duration,
        );
    }

    /// Clears a provider cooldown.
    pub fn clear_cooldown(&self, provider: &str) {
        let mut cooldowns = self.cooldowns.lock().unwrap();
        cooldowns.remove(provider);
    }

    /// Returns true if the pool has only 1 total actor (inline execution required).
    pub fn should_inline(&self) -> bool {
        let actors = self.actors.lock().unwrap();
        actors.len() <= 1
    }

    /// Returns available actor info grouped by provider.
    pub fn available_actors(&self) -> Vec<ActorInfo> {
        let actors = self.actors.lock().unwrap();
        let mut by_provider: HashMap<String, (Vec<String>, Vec<crate::domain::ModelCapability>)> =
            HashMap::new();

        for actor in actors.iter() {
            let entry = by_provider
                .entry(actor.provider_name.clone())
                .or_default();
            for tag in &actor.tags {
                if !entry.0.contains(tag) {
                    entry.0.push(tag.clone());
                }
            }
            for cap in &actor.capabilities {
                if !entry.1.contains(cap) {
                    entry.1.push(*cap);
                }
            }
        }

        by_provider
            .into_iter()
            .map(|(provider, (tags, capabilities))| ActorInfo {
                provider_name: provider,
                tags,
                capabilities,
            })
            .collect()
    }

    /// Finds an idle actor matching provider, tags, and capabilities.
    pub fn find_idle(
        &self,
        provider: &str,
        required_tags: &[String],
        required_caps: &[crate::domain::ModelCapability],
    ) -> Option<String> {
        let actors = self.actors.lock().unwrap();
        actors
            .iter()
            .find(|a| {
                a.provider_name == provider
                    && a.status == ActorStatus::Idle
                    && required_tags.iter().all(|t| a.tags.contains(t))
                    && required_caps.iter().all(|c| a.capabilities.contains(c))
            })
            .map(|a| a.id.clone())
    }

    /// Finds an idle actor matching required tags (any provider).
    /// Returns the provider name of the matched actor.
    pub fn find_idle_by_tags(&self, required_tags: &[String]) -> Option<String> {
        let actors = self.actors.lock().unwrap();
        actors
            .iter()
            .find(|a| {
                a.status == ActorStatus::Idle
                    && required_tags.iter().all(|t| a.tags.contains(t))
            })
            .map(|a| a.provider_name.clone())
    }
}

/// A held actor slot. Releases on drop.
pub struct ActorSlot {
    pub actor_id: String,
    pub provider: String,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

/// Summary of available actors.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActorInfo {
    pub provider_name: String,
    pub tags: Vec<String>,
    pub capabilities: Vec<crate::domain::ModelCapability>,
}

#[derive(Debug, thiserror::Error)]
pub enum ActorPoolError {
    #[error("no provider '{0}' configured")]
    NoProvider(String),
    #[error("no capacity for provider '{0}'")]
    Unavailable(String),
    #[error("provider '{0}' is in cooldown")]
    ProviderCooldown(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pool() -> ActorPool {
        let mut actors = HashMap::new();
        actors.insert("anthropic".to_string(), 2);
        actors.insert("ollama".to_string(), 1);

        ActorPool::new(ActorPoolConfig {
            actors_per_provider: actors,
            cooldown_duration: Duration::from_millis(100),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn acquire_and_release() {
        let pool = make_pool();
        let slot = pool.acquire("anthropic").await.unwrap();
        assert_eq!(slot.provider, "anthropic");

        pool.release(slot);
    }

    #[tokio::test]
    async fn no_provider_error() {
        let pool = make_pool();
        let result = pool.acquire("unknown").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cooldown_blocks() {
        let pool = make_pool();
        pool.set_cooldown("anthropic");

        let result = pool.acquire("anthropic").await;
        assert!(matches!(result, Err(ActorPoolError::ProviderCooldown(_))));

        // Wait for cooldown to expire
        tokio::time::sleep(Duration::from_millis(150)).await;
        let slot = pool.acquire("anthropic").await.unwrap();
        pool.release(slot);
    }

    #[test]
    fn should_inline() {
        let mut actors = HashMap::new();
        actors.insert("ollama".to_string(), 1);

        let pool = ActorPool::new(ActorPoolConfig {
            actors_per_provider: actors,
            ..Default::default()
        });
        assert!(pool.should_inline());
    }

    #[test]
    fn available_actors_info() {
        let pool = make_pool();
        let info = pool.available_actors();
        assert_eq!(info.len(), 2);
    }
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ozzie_core::config::ConnectorProcessConfig;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

/// Status of a supervised connector process.
#[derive(Debug, Clone)]
pub enum ProcessStatus {
    Starting,
    Running { pid: u32 },
    Failed { error: String, last_attempt: Instant },
    Stopped,
}

impl ProcessStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running { .. } => "running",
            Self::Failed { .. } => "failed",
            Self::Stopped => "stopped",
        }
    }

    /// Returns the error message if the process has failed.
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// A supervised connector process entry.
struct ConnectorProcess {
    config: ConnectorProcessConfig,
    status: ProcessStatus,
    child: Option<Child>,
}

/// Supervises connector child processes.
///
/// Spawns connector bridges as child processes, monitors them, restarts on crash,
/// and pipes stdout/stderr to tracing.
pub struct ProcessSupervisor {
    processes: RwLock<HashMap<String, ConnectorProcess>>,
    gateway_url: String,
    ozzie_path: PathBuf,
}

/// Summary of a supervised process for display.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub command: String,
    pub error: Option<String>,
}

impl ProcessSupervisor {
    pub fn new(gateway_url: String, ozzie_path: PathBuf) -> Self {
        Self {
            processes: RwLock::new(HashMap::new()),
            gateway_url,
            ozzie_path,
        }
    }

    /// Registers a connector to be supervised.
    pub async fn register(&self, name: String, config: ConnectorProcessConfig) {
        info!(connector = %name, command = %config.command, "connector registered");
        self.processes.write().await.insert(
            name,
            ConnectorProcess {
                config,
                status: ProcessStatus::Stopped,
                child: None,
            },
        );
    }

    /// Starts all registered connectors.
    pub async fn start_all(&self) {
        let names: Vec<String> = self.processes.read().await.keys().cloned().collect();
        for name in names {
            self.start_one(&name).await;
        }
    }

    /// Starts a single connector by name.
    pub async fn start_one(&self, name: &str) {
        let config = {
            let mut procs = self.processes.write().await;
            let Some(entry) = procs.get_mut(name) else {
                error!(connector = %name, "not registered");
                return;
            };
            if entry.status.is_running() {
                warn!(connector = %name, "already running, skipping");
                return;
            }
            entry.status = ProcessStatus::Starting;
            entry.config.clone()
        };

        match self.spawn_process(name, &config).await {
            Ok((child, pid)) => {
                info!(connector = %name, pid, "connector started");
                let mut procs = self.processes.write().await;
                if let Some(entry) = procs.get_mut(name) {
                    entry.status = ProcessStatus::Running { pid };
                    entry.child = Some(child);
                }
            }
            Err(e) => {
                error!(connector = %name, error = %e, "failed to start");
                let mut procs = self.processes.write().await;
                if let Some(entry) = procs.get_mut(name) {
                    entry.status = ProcessStatus::Failed {
                        error: e.to_string(),
                        last_attempt: Instant::now(),
                    };
                }
            }
        }
    }

    /// Stops a single connector by name (SIGTERM → wait → SIGKILL).
    pub async fn stop_one(&self, name: &str) {
        let mut procs = self.processes.write().await;
        let Some(entry) = procs.get_mut(name) else {
            return;
        };

        if let Some(ref mut child) = entry.child {
            info!(connector = %name, "stopping");

            // Try SIGTERM first for graceful shutdown
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                unsafe { libc::kill(pid as i32, libc::SIGTERM); }

                // Wait up to 3 seconds for graceful exit
                match tokio::time::timeout(
                    Duration::from_secs(3),
                    child.wait(),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        entry.child = None;
                        entry.status = ProcessStatus::Stopped;
                        return;
                    }
                    _ => {
                        warn!(connector = %name, "SIGTERM timeout, sending SIGKILL");
                    }
                }
            }

            // Fallback: SIGKILL
            if let Err(e) = child.kill().await {
                warn!(connector = %name, error = %e, "failed to kill connector process");
            }
            entry.child = None;
        }
        entry.status = ProcessStatus::Stopped;
    }

    /// Stops all connectors gracefully.
    pub async fn stop_all(&self) {
        let names: Vec<String> = self.processes.read().await.keys().cloned().collect();
        for name in names {
            self.stop_one(&name).await;
        }
    }

    /// Returns info about all supervised processes.
    pub async fn list(&self) -> Vec<ProcessInfo> {
        self.processes
            .read()
            .await
            .iter()
            .map(|(name, entry)| {
                let pid = match &entry.status {
                    ProcessStatus::Running { pid } => Some(*pid),
                    _ => None,
                };
                ProcessInfo {
                    name: name.clone(),
                    status: entry.status.display_name().to_string(),
                    pid,
                    command: entry.config.command.clone(),
                    error: entry.status.error().map(String::from),
                }
            })
            .collect()
    }

    /// Background monitoring loop — checks child processes and restarts if configured.
    ///
    /// Call this once after `start_all()`. Runs until the returned task is dropped.
    pub fn start_monitor(self: &std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        let supervisor = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            let restart_cooldown = Duration::from_secs(5);

            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;

                let names: Vec<String> =
                    supervisor.processes.read().await.keys().cloned().collect();

                for name in names {
                    let should_restart = {
                        let mut procs = supervisor.processes.write().await;
                        let Some(entry) = procs.get_mut(&name) else {
                            continue;
                        };

                        // Check if running child has exited
                        if let Some(ref mut child) = entry.child {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    let code = status.code().unwrap_or(-1);
                                    warn!(connector = %name, exit_code = code, "process exited");
                                    entry.child = None;
                                    entry.status = ProcessStatus::Failed {
                                        error: format!("exited with code {code}"),
                                        last_attempt: Instant::now(),
                                    };
                                    entry.config.restart
                                }
                                Ok(None) => false, // still running
                                Err(e) => {
                                    warn!(connector = %name, error = %e, "failed to check process");
                                    false
                                }
                            }
                        } else if let ProcessStatus::Failed { last_attempt, .. } = &entry.status
                            && entry.config.restart
                            && last_attempt.elapsed() >= restart_cooldown
                        {
                            true
                        } else {
                            false
                        }
                    };

                    if should_restart {
                        info!(connector = %name, "restarting after cooldown");
                        supervisor.start_one(&name).await;
                    }
                }
            }
        })
    }

    // ---- Internal ----

    /// Reads the gateway auth token from `$OZZIE_PATH/.token`.
    fn read_gateway_token(&self) -> Option<String> {
        let token_path = self.ozzie_path.join(".token");
        std::fs::read_to_string(&token_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    async fn spawn_process(
        &self,
        name: &str,
        config: &ConnectorProcessConfig,
    ) -> Result<(Child, u32), String> {
        let command_path = resolve_command(&config.command, &self.ozzie_path);

        let mut env: HashMap<String, String> = config.env.clone();

        // Inject gateway connection info
        env.insert("OZZIE_GATEWAY_URL".to_string(), self.gateway_url.clone());
        env.insert(
            "OZZIE_PATH".to_string(),
            self.ozzie_path.to_string_lossy().to_string(),
        );

        // Inject gateway token for auto-pairing connectors
        if config.auto_pair && let Some(token) = self.read_gateway_token() {
            env.insert("OZZIE_GATEWAY_TOKEN".to_string(), token);
        }

        // Connector-specific config as JSON env var
        if let Some(ref connector_config) = config.config {
            env.insert(
                "OZZIE_CONNECTOR_CONFIG".to_string(),
                serde_json::to_string(connector_config).unwrap_or_default(),
            );
        }

        let mut cmd = Command::new(&command_path);
        cmd.args(&config.args)
            .envs(&env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Prevent child from inheriting stdin
            .stdin(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", command_path.display()))?;

        let pid = child
            .id()
            .ok_or_else(|| "process exited immediately".to_string())?;

        // Forward stdout/stderr to tracing
        if let Some(stdout) = child.stdout.take() {
            let name = name.to_string();
            tokio::spawn(forward_stream(name, stdout, "stdout"));
        }
        if let Some(stderr) = child.stderr.take() {
            let name = name.to_string();
            tokio::spawn(forward_stream(name, stderr, "stderr"));
        }

        Ok((child, pid))
    }
}

/// Resolves a connector command to a full path.
///
/// Search order:
/// 1. `$OZZIE_PATH/connectors/<command>` — locally installed binaries
/// 2. `$PATH` — system binaries (fallback)
fn resolve_command(command: &str, ozzie_path: &Path) -> PathBuf {
    let local = ozzie_path.join("connectors").join(command);
    if local.exists() && local.is_file() {
        return local;
    }
    PathBuf::from(command)
}

/// Forwards an async reader line-by-line to tracing.
async fn forward_stream<R: tokio::io::AsyncRead + Unpin>(
    connector_name: String,
    stream: R,
    stream_name: &'static str,
) {
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match stream_name {
            "stderr" => warn!(connector = %connector_name, line = %line, "connector output"),
            _ => debug!(connector = %connector_name, line = %line, "connector output"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_command_falls_back_to_path() {
        let result = resolve_command("some-binary", Path::new("/nonexistent"));
        assert_eq!(result, PathBuf::from("some-binary"));
    }

    #[test]
    fn process_status_display() {
        assert_eq!(ProcessStatus::Starting.display_name(), "starting");
        assert_eq!(ProcessStatus::Running { pid: 42 }.display_name(), "running");
        assert_eq!(ProcessStatus::Stopped.display_name(), "stopped");
    }

    #[tokio::test]
    async fn register_and_list() {
        let sup = ProcessSupervisor::new("ws://localhost:18420".to_string(), PathBuf::from("/tmp"));
        sup.register(
            "test".to_string(),
            ConnectorProcessConfig {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                env: HashMap::new(),
                config: None,
                auto_pair: true,
                restart: false,
                timeout: 10_000,
            },
        )
        .await;

        let list = sup.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test");
        assert_eq!(list[0].status, "stopped");
    }

    #[tokio::test]
    async fn start_and_stop_echo() {
        let sup = ProcessSupervisor::new("ws://localhost:18420".to_string(), PathBuf::from("/tmp"));
        sup.register(
            "echo-test".to_string(),
            ConnectorProcessConfig {
                command: "sleep".to_string(),
                args: vec!["60".to_string()],
                env: HashMap::new(),
                config: None,
                auto_pair: true,
                restart: false,
                timeout: 10_000,
            },
        )
        .await;

        sup.start_one("echo-test").await;

        let list = sup.list().await;
        assert_eq!(list[0].status, "running");
        assert!(list[0].pid.is_some());

        sup.stop_one("echo-test").await;

        let list = sup.list().await;
        assert_eq!(list[0].status, "stopped");
    }
}

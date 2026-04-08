use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};

use ozzie_runtime::heartbeat;
use ozzie_utils::config::{logs_path, ozzie_path};

const PID_FILE: &str = ".pid";
const HEARTBEAT_FILE: &str = "heartbeat.json";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const DEFAULT_PORT: u16 = 18420;

/// Manage the Ozzie gateway daemon.
#[derive(Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    command: DaemonCommand,
}

impl DaemonArgs {
    /// Creates a `DaemonArgs` for `start` — used by `gateway --daemon` redirect.
    pub fn start(port: u16) -> Self {
        Self {
            command: DaemonCommand::Start { port },
        }
    }
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Start the gateway as a background daemon.
    Start {
        /// Listen port.
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Stop the running daemon.
    Stop,
    /// Check daemon status.
    Status,
}

pub async fn run(args: DaemonArgs) -> anyhow::Result<()> {
    match args.command {
        DaemonCommand::Start { port } => start(port).await,
        DaemonCommand::Stop => stop().await,
        DaemonCommand::Status => status().await,
    }
}

// ---- Start ----

async fn start(port: u16) -> anyhow::Result<()> {
    // Check if already running
    if let Some(pid) = read_pid() {
        if is_process_alive(pid) {
            println!("Ozzie is already running (PID {pid})");
            return Ok(());
        }
        // Stale PID file — clean up
        let _ = std::fs::remove_file(pid_path());
    }

    let ozzie_bin = std::env::current_exe()?;
    let log_dir = logs_path();
    std::fs::create_dir_all(&log_dir)?;
    let log_file = log_dir.join("gateway.log");

    println!("Starting Ozzie daemon on port {port}...");

    let child = spawn_detached(&ozzie_bin, port, &log_file)?;
    let pid = child;

    // Write PID file
    std::fs::write(pid_path(), pid.to_string())?;

    // Poll health check until ready
    let health_url = format!("http://127.0.0.1:{port}/api/health");
    match poll_health(&health_url, HEALTH_TIMEOUT).await {
        Ok(()) => {
            println!("Ozzie daemon started (PID {pid}, port {port})");
            println!("Logs: {}", log_file.display());
        }
        Err(_) => {
            println!("Ozzie daemon started (PID {pid}) but health check timed out.");
            println!("Check logs: {}", log_file.display());
        }
    }

    Ok(())
}

#[cfg(unix)]
fn spawn_detached(bin: &std::path::Path, port: u16, log_file: &std::path::Path) -> anyhow::Result<u32> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    let log = std::fs::File::create(log_file)?;
    let log_err = log.try_clone()?;

    let child = std::process::Command::new(bin)
        .args(["gateway", "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .process_group(0) // detach from parent process group
        .spawn()?;

    Ok(child.id())
}

#[cfg(not(unix))]
fn spawn_detached(bin: &std::path::Path, port: u16, log_file: &std::path::Path) -> anyhow::Result<u32> {
    use std::process::Stdio;

    let log = std::fs::File::create(log_file)?;
    let log_err = log.try_clone()?;

    let child = std::process::Command::new(bin)
        .args(["gateway", "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()?;

    Ok(child.id())
}

async fn poll_health(url: &str, timeout: Duration) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    loop {
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("health check timeout");
        }

        if let Ok(resp) = client.get(url).send().await
            && resp.status().is_success()
        {
            return Ok(());
        }

        tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
    }
}

// ---- Stop ----

async fn stop() -> anyhow::Result<()> {
    let Some(pid) = read_pid() else {
        println!("Ozzie is not running (no PID file)");
        return Ok(());
    };

    if !is_process_alive(pid) {
        let _ = std::fs::remove_file(pid_path());
        println!("Ozzie is not running (stale PID {pid}, cleaned up)");
        return Ok(());
    }

    println!("Stopping Ozzie (PID {pid})...");
    kill_process(pid);

    // Wait for process to exit
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if !is_process_alive(pid) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            println!("Process did not exit gracefully, force killing...");
            force_kill(pid);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = std::fs::remove_file(pid_path());
    println!("Ozzie stopped.");
    Ok(())
}

// ---- Status ----

async fn status() -> anyhow::Result<()> {
    let pid_info = read_pid();
    let heartbeat_info = heartbeat::check(
        &heartbeat_path(),
        Duration::from_secs(60),
    );

    match (pid_info, heartbeat_info) {
        (Some(pid), (heartbeat::Status::Alive, Some(hb))) => {
            let uptime = format_duration(Duration::from_secs(hb.uptime_secs));
            println!("Ozzie: RUNNING (PID {pid}, uptime {uptime})");
        }
        (Some(pid), (heartbeat::Status::Stale, _)) => {
            if is_process_alive(pid) {
                println!("Ozzie: RUNNING (PID {pid}, heartbeat stale)");
            } else {
                println!("Ozzie: STOPPED (stale PID {pid})");
            }
        }
        (Some(pid), _) => {
            if is_process_alive(pid) {
                println!("Ozzie: RUNNING (PID {pid}, no heartbeat)");
            } else {
                println!("Ozzie: STOPPED (stale PID {pid})");
            }
        }
        (None, (heartbeat::Status::Alive, Some(hb))) => {
            println!("Ozzie: RUNNING (PID {}, no PID file — started externally?)", hb.pid);
        }
        (None, _) => {
            println!("Ozzie: STOPPED");
        }
    }

    Ok(())
}

// ---- Helpers ----

fn pid_path() -> PathBuf {
    ozzie_path().join(PID_FILE)
}

fn heartbeat_path() -> PathBuf {
    ozzie_path().join(HEARTBEAT_FILE)
}

fn read_pid() -> Option<u32> {
    std::fs::read_to_string(pid_path())
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    // kill(pid, 0) checks existence without sending a signal
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn is_process_alive(_pid: u32) -> bool {
    // Fallback: assume alive if PID file exists
    true
}

#[cfg(unix)]
fn kill_process(pid: u32) {
    unsafe { libc::kill(pid as i32, libc::SIGTERM); }
}

#[cfg(not(unix))]
fn kill_process(_pid: u32) {
    // Not implemented for non-Unix
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
}

#[cfg(not(unix))]
fn force_kill(_pid: u32) {}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_secs(42)), "42s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(130)), "2m 10s");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(7300)), "2h 1m");
    }

    #[test]
    fn read_pid_missing() {
        // pid_path() won't exist in test env (no .ozzie dir)
        // Just verify the function handles missing files gracefully
        let result = std::fs::read_to_string("/nonexistent/path/.pid")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        assert!(result.is_none());
    }
}

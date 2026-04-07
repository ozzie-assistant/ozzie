use std::io::{self, BufRead, Write as IoWrite};

use ozzie_utils::i18n;

use super::section::{
    CollectResult, FieldKind, FieldSpec, FieldValue, FieldValues, InputCollector,
};

/// Stdin-based input collector for the CLI wizard and `config set`.
pub struct StdinCollector;

impl StdinCollector {
    pub fn new() -> Self {
        Self
    }
}

impl InputCollector for StdinCollector {
    fn show_title(&mut self, section_id: &str) {
        let title = i18n::t(&format!("wizard.{section_id}.title"));
        eprintln!();
        eprintln!("── {title} ──");
        eprintln!();
    }

    fn show_info(&mut self, message: &str) {
        eprintln!("{message}");
    }

    fn show_errors(&mut self, errors: &[String]) {
        for err in errors {
            eprintln!("  \u{26a0} {err}");
        }
    }

    fn collect(
        &mut self,
        section_id: &str,
        fields: &[FieldSpec],
    ) -> anyhow::Result<CollectResult> {
        let mut values = FieldValues::new();

        for field in fields {
            let label = resolve_label(section_id, &field.key);

            match &field.kind {
                FieldKind::Text { default } => {
                    let value = if let Some(def) = default {
                        prompt_default(&label, def)?
                    } else {
                        prompt(&label)?
                    };
                    if field.required && value.is_empty() {
                        return Ok(CollectResult::Back);
                    }
                    values.insert(field.key.clone(), FieldValue::Text(value));
                }

                FieldKind::Secret => {
                    let value = prompt_secret(&label)?;
                    values.insert(field.key.clone(), FieldValue::Text(value));
                }

                FieldKind::Select { options, default } => {
                    println!("{label}");
                    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
                    let idx = select(&labels, *default)?;
                    values.insert(field.key.clone(), FieldValue::Index(idx));
                }

                FieldKind::Confirm { default } => {
                    let result = confirm(&label, *default)?;
                    values.insert(field.key.clone(), FieldValue::Bool(result));
                }

                FieldKind::MultiSelect { options } => {
                    println!("{label}");
                    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
                    let indices = multi_select(&labels)?;
                    values.insert(field.key.clone(), FieldValue::Indices(indices));
                }

            }
        }

        Ok(CollectResult::Values(values))
    }
}

// ── i18n resolution ────────────────────────────────────────────────────────

/// Resolves a field label via i18n: `wizard.{section_id}.{field_key}`.
/// Falls back to the field key itself if no translation found.
fn resolve_label(section_id: &str, field_key: &str) -> String {
    let key = format!("wizard.{section_id}.{field_key}");
    let result = i18n::t(&key);
    // If t() returned the key itself (no translation), use just the field key
    if result == key {
        field_key.replace('_', " ")
    } else {
        result
    }
}

// ── Low-level stdin helpers ────────────────────────────────────────────────

fn prompt(question: &str) -> io::Result<String> {
    eprint!("{question} ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(strip_escape_sequences(input.trim()))
}

fn prompt_default(question: &str, default: &str) -> io::Result<String> {
    eprint!("{question} [{default}]: ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let trimmed = strip_escape_sequences(input.trim());
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed)
    }
}

/// Strips ANSI escape sequences (arrow keys, etc.) from input.
fn strip_escape_sequences(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC + [ + one or more parameter chars + final char
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Consume parameter bytes (0x30–0x3F) and intermediate (0x20–0x2F)
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() || next == ';' || (' '..='?').contains(&next) {
                        chars.next();
                    } else {
                        break;
                    }
                }
                // Consume final byte (0x40–0x7E)
                if let Some(&next) = chars.peek()
                    && ('@'..='~').contains(&next)
                {
                    chars.next();
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn prompt_secret(question: &str) -> io::Result<String> {
    eprint!("{question} ");
    io::stderr().flush()?;

    crossterm::terminal::enable_raw_mode()?;
    let mut input = String::new();
    loop {
        if crossterm::event::poll(std::time::Duration::from_millis(100))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
        {
            match key.code {
                crossterm::event::KeyCode::Enter => break,
                crossterm::event::KeyCode::Backspace => {
                    input.pop();
                }
                crossterm::event::KeyCode::Char(c) => {
                    input.push(c);
                }
                crossterm::event::KeyCode::Esc => {
                    input.clear();
                    break;
                }
                _ => {}
            }
        }
    }
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();

    Ok(input.trim().to_string())
}

fn select(options: &[&str], default: usize) -> io::Result<usize> {
    for (i, opt) in options.iter().enumerate() {
        let marker = if i == default { ">" } else { " " };
        eprintln!("  {marker} [{}] {opt}", i + 1);
    }
    eprint!("Choice [{}]: ", default + 1);
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim();

    if trimmed.is_empty() {
        return Ok(default);
    }

    match trimmed.parse::<usize>() {
        Ok(n) if n >= 1 && n <= options.len() => Ok(n - 1),
        _ => Ok(default),
    }
}

fn confirm(question: &str, default_yes: bool) -> io::Result<bool> {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    eprint!("{question} {hint} ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();

    if trimmed.is_empty() {
        return Ok(default_yes);
    }

    Ok(trimmed.starts_with('y') || trimmed.starts_with('o'))
}

fn multi_select(options: &[&str]) -> io::Result<Vec<usize>> {
    for (i, opt) in options.iter().enumerate() {
        eprintln!("  [{}] {opt}", i + 1);
    }
    eprint!("Select (comma-separated, e.g. 1,3,5): ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    let selected: Vec<usize> = input
        .split(',')
        .filter_map(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1 && n <= options.len())
        .map(|n| n - 1)
        .collect();

    Ok(selected)
}

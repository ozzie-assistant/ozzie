use comfy_table::{presets::UTF8_FULL_CONDENSED, Cell, CellAlignment, Table};
use serde::Serialize;

/// Prints a formatted table to stdout.
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(headers.iter().map(|h| {
        Cell::new(h).set_alignment(CellAlignment::Left)
    }));

    for row in rows {
        table.add_row(row);
    }

    println!("{table}");
}

/// Prints data as JSON to stdout.
pub fn print_json<T: Serialize>(data: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(data)?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_does_not_panic() {
        print_table(
            &["NAME", "STATUS"],
            vec![vec!["test".into(), "ok".into()]],
        );
    }

    #[test]
    fn json_output() {
        let data = serde_json::json!({"key": "value"});
        print_json(&data).unwrap();
    }
}

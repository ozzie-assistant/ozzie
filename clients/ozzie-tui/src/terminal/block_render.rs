use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::block::{AssistantBlock, Block, BlockState, SystemBlock, ToolCallBlock, UserBlock};
use crate::render::timestamp::format_ts;
use crate::render::RenderContext;

/// Maximum number of consecutive tool calls rendered individually.
/// Beyond this, a condensed summary is shown.
const TOOL_EXPAND_LIMIT: usize = 4;

/// Renders a block into a list of styled lines.
pub fn render_block(block: &Block, selected: bool, ctx: &RenderContext) -> Vec<Line<'static>> {
    match block {
        Block::User(b) => render_user(b, selected, ctx),
        Block::Assistant(b) => render_assistant(b, selected, ctx),
        Block::ToolCall(b) => render_tool_call(b, selected),
        Block::System(b) => render_system(b, selected, ctx),
    }
}

/// Renders a run of consecutive tool call blocks.
/// If the run is short (≤ TOOL_EXPAND_LIMIT), each tool gets its own line with detail.
/// If the run is long, a condensed summary is shown.
pub fn render_tool_run(blocks: &[&ToolCallBlock], selected_idx: Option<usize>) -> Vec<Line<'static>> {
    if blocks.len() <= TOOL_EXPAND_LIMIT {
        let mut lines = Vec::new();
        for (i, block) in blocks.iter().enumerate() {
            let sel = selected_idx == Some(i);
            lines.extend(render_tool_call(block, sel));
        }
        return lines;
    }

    // Condensed view: group by status
    let total = blocks.len();
    let done = blocks.iter().filter(|b| b.state == BlockState::Finalized && !b.is_error).count();
    let errors = blocks.iter().filter(|b| b.is_error).count();
    let pending = total - done - errors;

    let mut spans = vec![
        Span::styled("  ⎿  ", Style::default().fg(Color::DarkGray)),
    ];

    // Summary: "5 tools ✓ 4  ✗ 1" or "5 tools ⏳ 2  ✓ 3"
    spans.push(Span::styled(
        format!("{total} tools"),
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    ));

    if done > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("✓", Style::default().fg(Color::Green)));
        spans.push(Span::styled(
            format!(" {done}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if errors > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("✗", Style::default().fg(Color::Red)));
        spans.push(Span::styled(
            format!(" {errors}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if pending > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("⏳", Style::default().fg(Color::Yellow)));
        spans.push(Span::styled(
            format!(" {pending}"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let mut lines = vec![Line::from(spans)];

    // List tool names compactly
    let names: Vec<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
    let name_line = format!("     {}", names.join(", "));
    lines.push(Line::from(Span::styled(
        name_line,
        Style::default().fg(Color::DarkGray),
    )));

    lines
}

fn base_style(selected: bool) -> Style {
    if selected {
        Style::default().bg(Color::DarkGray)
    } else {
        Style::default()
    }
}

fn render_user(block: &UserBlock, selected: bool, ctx: &RenderContext) -> Vec<Line<'static>> {
    let style = base_style(selected);
    let ts = format_ts(block.ts, ctx.language.as_deref());
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
        Span::styled(
            "❯ You",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .style(style)];

    for content_line in block.content.lines() {
        lines.push(Line::from(format!("  {content_line}")).style(style));
    }
    lines.push(Line::from(""));
    lines
}

fn render_assistant(
    block: &AssistantBlock,
    selected: bool,
    ctx: &RenderContext,
) -> Vec<Line<'static>> {
    let style = base_style(selected);
    let ts = format_ts(block.ts, ctx.language.as_deref());
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
        Span::styled(
            "✦ Ozzie",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .style(style)];

    // Render content as markdown
    let md_lines = crate::render::markdown::render_markdown(&block.content, "  ");
    for line in md_lines {
        lines.push(line.style(style));
    }

    if block.state == BlockState::Active {
        lines.push(
            Line::from(Span::styled("  ▊", Style::default().fg(Color::Cyan))).style(style),
        );
    }
    lines.push(Line::from(""));
    lines
}

fn render_tool_call(block: &ToolCallBlock, selected: bool) -> Vec<Line<'static>> {
    let style = base_style(selected);

    let status_icon = match (block.state, block.is_error) {
        (BlockState::Finalized, true) => Span::styled("✗", Style::default().fg(Color::Red)),
        (BlockState::Finalized, false) => Span::styled("✓", Style::default().fg(Color::Green)),
        _ => Span::styled("⏳", Style::default().fg(Color::Yellow)),
    };

    // Extract a short detail from arguments
    let detail = extract_tool_detail(&block.name, &block.arguments);

    if block.collapsed {
        let mut spans = vec![
            Span::styled("  ⎿ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                block.name.clone(),
                Style::default().fg(Color::Magenta),
            ),
        ];
        if !detail.is_empty() {
            spans.push(Span::styled(
                format!(" {detail}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        spans.push(Span::raw(" "));
        spans.push(status_icon);

        vec![Line::from(spans).style(style)]
    } else {
        // Header
        let mut header_spans = vec![
            Span::styled("  ⎿ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                block.name.clone(),
                Style::default().fg(Color::Magenta),
            ),
        ];
        if !detail.is_empty() {
            header_spans.push(Span::styled(
                format!(" {detail}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        header_spans.push(Span::raw(" "));
        header_spans.push(status_icon);

        let mut lines = vec![Line::from(header_spans).style(style)];

        // Show result if available
        match &block.result {
            Some(result) if !result.is_empty() => {
                let truncated = truncate_output(result, "    ");
                for line in truncated {
                    lines.push(line.style(style));
                }
            }
            _ => {}
        }

        lines
    }
}

/// Extracts a short human-readable detail from tool arguments JSON.
fn extract_tool_detail(tool_name: &str, arguments: &str) -> String {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return String::new();
    };

    // Try common field names in priority order depending on tool type
    let detail = match tool_name {
        name if name.contains("search") || name.contains("web") => {
            args.get("query")
                .or_else(|| args.get("q"))
                .and_then(|v| v.as_str())
                .map(|s| truncate_str(s, 50))
        }
        name if name.contains("read") || name.contains("write") || name.contains("file")
            || name.contains("glob") || name.contains("grep") => {
            args.get("path")
                .or_else(|| args.get("file_path"))
                .or_else(|| args.get("pattern"))
                .and_then(|v| v.as_str())
                .map(|s| truncate_str(s, 60))
        }
        name if name.contains("cmd") || name.contains("bash") || name.contains("exec") => {
            args.get("command")
                .or_else(|| args.get("cmd"))
                .and_then(|v| v.as_str())
                .map(|s| truncate_str(s, 60))
        }
        name if name.contains("memory") || name.contains("remember") => {
            args.get("query")
                .or_else(|| args.get("title"))
                .or_else(|| args.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| truncate_str(s, 50))
        }
        _ => {
            // Generic: pick the first string field value
            first_string_value(&args, 50)
        }
    };

    detail.unwrap_or_default()
}

/// Returns the first string value from a JSON object, truncated.
fn first_string_value(val: &serde_json::Value, max: usize) -> Option<String> {
    let obj = val.as_object()?;
    for v in obj.values() {
        if let Some(s) = v.as_str()
            && !s.is_empty()
        {
            return Some(truncate_str(s, max));
        }
    }
    None
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Truncates long output: 5 head lines + "… +N lines" + 5 tail lines.
fn truncate_output(content: &str, indent: &str) -> Vec<Line<'static>> {
    const MAX_HEAD: usize = 5;
    const MAX_TAIL: usize = 5;
    let all: Vec<&str> = content.lines().collect();
    let dim = Style::default().fg(Color::DarkGray);

    if all.len() <= MAX_HEAD + MAX_TAIL + 1 {
        return all
            .iter()
            .map(|l| Line::from(format!("{indent}{l}")).style(dim))
            .collect();
    }

    let mut lines = Vec::new();
    for l in &all[..MAX_HEAD] {
        lines.push(Line::from(format!("{indent}{l}")).style(dim));
    }
    let omitted = all.len() - MAX_HEAD - MAX_TAIL;
    lines.push(
        Line::from(format!("{indent}… +{omitted} lines"))
            .style(Style::default().fg(Color::Rgb(100, 100, 100))),
    );
    for l in &all[all.len() - MAX_TAIL..] {
        lines.push(Line::from(format!("{indent}{l}")).style(dim));
    }
    lines
}

fn render_system(
    block: &SystemBlock,
    selected: bool,
    ctx: &RenderContext,
) -> Vec<Line<'static>> {
    let style = base_style(selected);
    let ts = format_ts(block.ts, ctx.language.as_deref());
    vec![
        Line::from(vec![
            Span::styled(format!("{ts} "), Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("⚠ {}", block.content),
                Style::default().fg(Color::Yellow),
            ),
        ])
        .style(style),
        Line::from(""),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn user_block_renders() {
        let ctx = RenderContext::default();
        let block = Block::User(UserBlock {
            id: 0,
            ts: Utc::now(),
            content: "hello".to_string(),
        });
        let lines = render_block(&block, false, &ctx);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn tool_call_collapsed_vs_expanded() {
        let mut tc = ToolCallBlock {
            id: 0,
            ts: Utc::now(),
            call_id: "tc_1".to_string(),
            name: "file_read".to_string(),
            arguments: "{}".to_string(),
            result: None,
            is_error: false,
            collapsed: true,
            state: BlockState::Active,
        };
        let collapsed_lines = render_tool_call(&tc, false);

        tc.collapsed = false;
        tc.result = Some("some content".to_string());
        let expanded_lines = render_tool_call(&tc, false);

        assert!(expanded_lines.len() > collapsed_lines.len());
    }

    #[test]
    fn tool_call_with_detail() {
        let tc = ToolCallBlock {
            id: 0,
            ts: Utc::now(),
            call_id: "tc_1".to_string(),
            name: "web_search".to_string(),
            arguments: r#"{"query":"weather in Paris"}"#.to_string(),
            result: None,
            is_error: false,
            collapsed: true,
            state: BlockState::Active,
        };
        let lines = render_tool_call(&tc, false);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("weather in Paris"));
    }

    #[test]
    fn tool_call_error_shows_red() {
        let tc = ToolCallBlock {
            id: 0,
            ts: Utc::now(),
            call_id: "tc_3".to_string(),
            name: "cmd".to_string(),
            arguments: "{}".to_string(),
            result: Some("Error: not found".to_string()),
            is_error: true,
            collapsed: true,
            state: BlockState::Finalized,
        };
        let lines = render_tool_call(&tc, false);
        let has_red = lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Red));
        assert!(has_red);
    }

    #[test]
    fn tool_run_condensed() {
        let blocks: Vec<ToolCallBlock> = (0..6)
            .map(|i| ToolCallBlock {
                id: i,
                ts: Utc::now(),
                call_id: format!("tc_{i}"),
                name: format!("tool_{i}"),
                arguments: "{}".to_string(),
                result: Some("ok".to_string()),
                is_error: false,
                collapsed: true,
                state: BlockState::Finalized,
            })
            .collect();
        let refs: Vec<&ToolCallBlock> = blocks.iter().collect();
        let lines = render_tool_run(&refs, None);
        // Condensed: summary line + names line = 2 lines (not 6)
        assert_eq!(lines.len(), 2);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("6 tools"));
    }

    #[test]
    fn tool_run_expanded() {
        let blocks: Vec<ToolCallBlock> = (0..3)
            .map(|i| ToolCallBlock {
                id: i,
                ts: Utc::now(),
                call_id: format!("tc_{i}"),
                name: format!("tool_{i}"),
                arguments: "{}".to_string(),
                result: None,
                is_error: false,
                collapsed: true,
                state: BlockState::Active,
            })
            .collect();
        let refs: Vec<&ToolCallBlock> = blocks.iter().collect();
        let lines = render_tool_run(&refs, None);
        // 3 tools, each 1 line collapsed = 3 lines
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn extract_detail_search() {
        let d = extract_tool_detail("web_search", r#"{"query":"hello world"}"#);
        assert_eq!(d, "hello world");
    }

    #[test]
    fn extract_detail_file() {
        let d = extract_tool_detail("file_read", r#"{"path":"/tmp/foo.rs"}"#);
        assert_eq!(d, "/tmp/foo.rs");
    }

    #[test]
    fn extract_detail_cmd() {
        let d = extract_tool_detail("cmd", r#"{"command":"ls -la"}"#);
        assert_eq!(d, "ls -la");
    }

    #[test]
    fn extract_detail_generic() {
        let d = extract_tool_detail("unknown_tool", r#"{"foo":"bar"}"#);
        assert_eq!(d, "bar");
    }

    #[test]
    fn extract_detail_empty() {
        let d = extract_tool_detail("tool", "{}");
        assert!(d.is_empty());
    }

    #[test]
    fn truncate_short_output() {
        let lines = truncate_output("line1\nline2\nline3", "  ");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn truncate_long_output() {
        let content: String = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let lines = truncate_output(&content, "  ");
        // 5 head + 1 omitted + 5 tail = 11
        assert_eq!(lines.len(), 11);
    }
}

//! XML-based tool calling shim for models without native function calling.
//!
//! Provides:
//! - [`parse_xml_tool_calls`]: parses `<function=NAME>...</function>` from LLM output
//! - [`tool_prompt_template`]: generates a system prompt section describing tools in XML format
//!
//! Used as fallback when models (Qwen, GLM-4, small SLMs via Ollama) emit tool
//! calls as XML in the content field instead of structured `tool_calls`.

use crate::{ToolCall, ToolDefinition};

/// Parses XML-style tool calls from content.
///
/// Returns `Some((tool_calls, remaining_text))` if at least one call was
/// parsed, `None` otherwise.
///
/// Format (opening `<tool_call>` and closing `</tool_call>` are optional):
/// ```text
/// <function=TOOL_NAME>
/// <parameter=KEY>
/// VALUE (may span multiple lines)
/// </parameter>
/// </function>
/// ```
pub fn parse_xml_tool_calls(content: &str) -> Option<(Vec<ToolCall>, String)> {
    // Quick check — avoid the parsing machinery for the common (no-XML) case.
    if !content.contains("<function=") {
        return None;
    }

    let mut calls = Vec::new();
    let mut remaining = String::new();
    let mut cursor = 0;

    while let Some(fn_start) = content[cursor..].find("<function=") {
        // Text before this function tag is kept as remaining content,
        // but strip any leading <tool_call> wrapper tag.
        let prefix = &content[cursor..cursor + fn_start];
        if let Some(tc_pos) = prefix.rfind("<tool_call>") {
            remaining.push_str(prefix[..tc_pos].trim_end());
        } else {
            remaining.push_str(prefix);
        }
        let fn_start_abs = cursor + fn_start;

        // Extract tool name: <function=NAME>
        let after_eq = fn_start_abs + "<function=".len();
        let name_end = match content[after_eq..].find('>') {
            Some(i) => after_eq + i,
            None => break,
        };
        let name = content[after_eq..name_end].trim().to_string();
        if name.is_empty() {
            break;
        }

        // Find the closing </function>
        let body_start = name_end + 1;
        let fn_end = match content[body_start..].find("</function>") {
            Some(i) => body_start + i,
            None => break,
        };
        let body = &content[body_start..fn_end];

        // Extract parameters: <parameter=KEY>VALUE</parameter>
        let mut args = serde_json::Map::new();
        let mut param_cursor = 0;
        while let Some(ps) = body[param_cursor..].find("<parameter=") {
            let ps_abs = param_cursor + ps;
            let after_p_eq = ps_abs + "<parameter=".len();
            let key_end = match body[after_p_eq..].find('>') {
                Some(i) => after_p_eq + i,
                None => break,
            };
            let key = body[after_p_eq..key_end].trim().to_string();

            let val_start = key_end + 1;
            let val_end = match body[val_start..].find("</parameter>") {
                Some(i) => val_start + i,
                None => break,
            };
            let val = body[val_start..val_end].trim();

            // Try parsing as JSON value first (handles numbers, bools, arrays, objects).
            let json_val = serde_json::from_str(val)
                .unwrap_or(serde_json::Value::String(val.to_string()));
            args.insert(key, json_val);

            param_cursor = val_end + "</parameter>".len();
        }

        // Generate a random ID (32 alphanumeric chars, matching llama.cpp style).
        let id: String = (0..32)
            .map(|_| {
                let idx = rand::random::<u8>() % 62;
                (match idx {
                    0..=9 => b'0' + idx,
                    10..=35 => b'a' + idx - 10,
                    _ => b'A' + idx - 36,
                }) as char
            })
            .collect();

        calls.push(ToolCall {
            id,
            name,
            arguments: serde_json::Value::Object(args),
        });

        // Advance past </function> and optional </tool_call>
        cursor = fn_end + "</function>".len();
        let after_fn = content[cursor..].trim_start();
        if after_fn.starts_with("</tool_call>") {
            cursor = content.len() - after_fn.len() + "</tool_call>".len();
        }
    }

    // Append any trailing text after the last parsed call.
    if cursor < content.len() {
        remaining.push_str(&content[cursor..]);
    }

    if calls.is_empty() {
        None
    } else {
        Some((calls, remaining.trim().to_string()))
    }
}

/// Generates a system prompt section that teaches the model the XML tool call format.
///
/// Injected into the system prompt when the model doesn't support native tool calling.
pub fn tool_prompt_template(tools: &[ToolDefinition]) -> String {
    let mut out = String::from(
        "# Tool Calling\n\n\
         You have access to the following tools. To call a tool, use this XML format:\n\n\
         ```\n\
         <function=TOOL_NAME>\n\
         <parameter=PARAM_NAME>value</parameter>\n\
         </function>\n\
         ```\n\n\
         You may call multiple tools in a single response. \
         Always use the exact tool and parameter names shown below.\n\n\
         ## Available Tools\n\n",
    );

    for tool in tools {
        out.push_str(&format!("### {}\n", tool.name));
        out.push_str(&tool.description);
        out.push('\n');

        // Extract parameter info from JSON Schema
        if let Some(obj) = tool.parameters.schema.object.as_ref()
            && !obj.properties.is_empty()
        {
            let required: std::collections::HashSet<&str> =
                obj.required.iter().map(|s| s.as_str()).collect();

            out.push_str("Parameters:\n");
            for (name, schema) in &obj.properties {
                let desc = schema_description(schema);
                let tag = if required.contains(name.as_str()) {
                    " (required)"
                } else {
                    " (optional)"
                };
                out.push_str(&format!("- `{name}`{tag}: {desc}\n"));
            }
        }
        out.push('\n');
    }

    out
}

/// Extracts a human-readable description from a JSON Schema object.
fn schema_description(schema: &schemars::schema::Schema) -> String {
    match schema {
        schemars::schema::Schema::Object(obj) => obj
            .metadata
            .as_ref()
            .and_then(|m| m.description.clone())
            .unwrap_or_else(|| {
                // Fallback: show the type if available
                obj.instance_type
                    .as_ref()
                    .map(|t| format!("{t:?}"))
                    .unwrap_or_else(|| "any".to_string())
            }),
        schemars::schema::Schema::Bool(_) => "any".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_with_tool_call_wrapper() {
        let content = "<tool_call>\n<function=store_memory>\n<parameter=content>\nHello world\n</parameter>\n<parameter=type>\nnote\n</parameter>\n</function>\n</tool_call>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "store_memory");
        assert_eq!(calls[0].arguments["content"], "Hello world");
        assert_eq!(calls[0].arguments["type"], "note");
        assert!(remaining.is_empty());
    }

    #[test]
    fn xml_without_tool_call_wrapper() {
        let content = "<function=store_memory>\n<parameter=content>\nclé ABC\n</parameter>\n<parameter=type>\nmemo\n</parameter>\n</function>\n</tool_call>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "store_memory");
        assert_eq!(calls[0].arguments["content"], "clé ABC");
        assert_eq!(calls[0].arguments["type"], "memo");
        assert!(remaining.is_empty());
    }

    #[test]
    fn xml_with_reasoning_before() {
        let content = "Let me store this in memory.\n\n<function=store_memory>\n<parameter=content>\ntest\n</parameter>\n</function>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "store_memory");
        assert_eq!(remaining, "Let me store this in memory.");
    }

    #[test]
    fn xml_multiline_value() {
        let content = "<function=file_write>\n<parameter=path>\n/tmp/test.txt\n</parameter>\n<parameter=content>\nLine 1\nLine 2\nLine 3\n</parameter>\n</function>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments["content"], "Line 1\nLine 2\nLine 3");
        assert!(remaining.is_empty());
    }

    #[test]
    fn xml_multiple_calls() {
        let content = "<function=file_read>\n<parameter=path>\na.txt\n</parameter>\n</function>\n<function=file_read>\n<parameter=path>\nb.txt\n</parameter>\n</function>";
        let (calls, _) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments["path"], "a.txt");
        assert_eq!(calls[1].arguments["path"], "b.txt");
    }

    #[test]
    fn no_xml_returns_none() {
        assert!(parse_xml_tool_calls("Just a normal response.").is_none());
        assert!(parse_xml_tool_calls("").is_none());
    }

    #[test]
    fn malformed_xml_returns_none() {
        assert!(
            parse_xml_tool_calls("<function=test>\n<parameter=a>\nval\n</parameter>").is_none()
        );
    }

    #[test]
    fn json_parameter_value() {
        let content = "<function=calculator>\n<parameter=numbers>\n[1, 2, 3]\n</parameter>\n<parameter=count>\n42\n</parameter>\n</function>";
        let (calls, _) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls[0].arguments["numbers"], serde_json::json!([1, 2, 3]));
        assert_eq!(calls[0].arguments["count"], serde_json::json!(42));
    }

    #[test]
    fn unique_ids_generated() {
        let content = "<function=a>\n</function>\n<function=b>\n</function>";
        let (calls, _) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].id, calls[1].id);
        assert_eq!(calls[0].id.len(), 32);
    }

    #[test]
    fn template_includes_tool_info() {
        let tools = vec![ToolDefinition {
            name: "file_read".to_string(),
            description: "Read a file from disk.".to_string(),
            parameters: schemars::schema_for!(FileReadArgs),
        }];
        let template = tool_prompt_template(&tools);
        assert!(template.contains("# Tool Calling"));
        assert!(template.contains("### file_read"));
        assert!(template.contains("Read a file from disk."));
        assert!(template.contains("`path`"));
        assert!(template.contains("(required)"));
    }

    #[test]
    fn template_empty_tools() {
        let template = tool_prompt_template(&[]);
        assert!(template.contains("# Tool Calling"));
        assert!(!template.contains("###"));
    }

    /// Test fixture for template tests.
    #[derive(schemars::JsonSchema)]
    struct FileReadArgs {
        /// Path to the file to read.
        path: String,
        /// Maximum number of lines to return.
        #[allow(dead_code)]
        max_lines: Option<u32>,
    }
}

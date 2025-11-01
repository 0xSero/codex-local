use serde_json::{json, Value};

/// Transform MiniMax-style XML tool calls into standard JSON ResponseItem format.
///
/// This function serves as a **fallback parser** for MiniMax models that output XML
/// tool calls despite being instructed to use JSON format. The system prompt now
/// explicitly requests JSON format, but this transformer ensures compatibility if
/// the model reverts to its native XML format.
///
/// MiniMax models may output tool calls in XML format:
/// ```xml
/// <minimax:tool_call>
/// <invoke name="function_name">
/// <parameter name="param1">value1</parameter>
/// <parameter name="param2">value2</parameter>
/// </invoke>
/// </minimax:tool_call>
/// ```
///
/// This function detects such XML blocks and converts them to the expected JSON format:
/// ```json
/// {
///   "type": "function_call",
///   "name": "function_name",
///   "arguments": "{\"param1\":\"value1\",\"param2\":\"value2\"}",
///   "call_id": "minimax_call_<uuid>"
/// }
/// ```
pub(crate) fn transform_minimax_tool_calls(item_value: &mut Value) -> bool {
    let mut transformed = false;

    // Check if this is a message with text content that might contain XML tool calls
    if let Some(content_array) = item_value.get_mut("content").and_then(|v| v.as_array_mut()) {
        for content_item in content_array.iter_mut() {
            if let Some(text) = content_item.get_mut("text").and_then(|v| v.as_str()) {
                if text.contains("<minimax:tool_call>") {
                    // Extract and transform XML tool calls
                    if let Some(transformed_items) = extract_minimax_tool_calls(text) {
                        // Replace the message content with the extracted tool calls
                        *item_value = transformed_items;
                        transformed = true;
                        break;
                    }
                }
            }
        }
    }

    transformed
}

/// Extract MiniMax XML tool calls from text and convert to ResponseItem JSON.
fn extract_minimax_tool_calls(text: &str) -> Option<Value> {
    let mut tool_calls = Vec::new();
    let mut remaining_text = String::new();
    let mut pos = 0;

    while pos < text.len() {
        if let Some(start) = text[pos..].find("<minimax:tool_call>") {
            let abs_start = pos + start;

            // Add any text before the tool call to remaining_text
            if start > 0 {
                remaining_text.push_str(&text[pos..abs_start]);
            }

            // Find the closing tag
            if let Some(end_rel) = text[abs_start..].find("</minimax:tool_call>") {
                let abs_end = abs_start + end_rel + "</minimax:tool_call>".len();
                let xml_block = &text[abs_start..abs_end];

                // Parse the XML block
                if let Some(tool_call) = parse_minimax_xml_block(xml_block) {
                    tool_calls.push(tool_call);
                }

                pos = abs_end;
            } else {
                // Unclosed tag, treat rest as text
                remaining_text.push_str(&text[abs_start..]);
                break;
            }
        } else {
            // No more tool calls, add rest of text
            remaining_text.push_str(&text[pos..]);
            break;
        }
    }

    if !tool_calls.is_empty() {
        // Return the first tool call (codex-local handles one at a time)
        // If there's remaining text, we'll need to handle it separately
        Some(tool_calls.into_iter().next().unwrap())
    } else {
        None
    }
}

/// Parse a single MiniMax XML tool call block.
fn parse_minimax_xml_block(xml: &str) -> Option<Value> {
    // Extract the invoke block
    let invoke_start = xml.find("<invoke name=\"")?;
    let name_start = invoke_start + "<invoke name=\"".len();
    let name_end = xml[name_start..].find('"')?;
    let function_name = &xml[name_start..name_start + name_end];

    // Find the invoke closing tag
    let invoke_content_start = xml[name_start + name_end..].find('>')?;
    let content_start = name_start + name_end + invoke_content_start + 1;

    let invoke_end = xml.find("</invoke>")?;
    let invoke_content = &xml[content_start..invoke_end];

    // Parse parameters
    let mut arguments = serde_json::Map::new();
    let mut param_pos = 0;

    while param_pos < invoke_content.len() {
        if let Some(param_start) = invoke_content[param_pos..].find("<parameter name=\"") {
            let abs_param_start = param_pos + param_start;
            let param_name_start = abs_param_start + "<parameter name=\"".len();

            if let Some(param_name_end) = invoke_content[param_name_start..].find('"') {
                let param_name = &invoke_content[param_name_start..param_name_start + param_name_end];

                // Find the closing >
                if let Some(param_value_start_rel) = invoke_content[param_name_start + param_name_end..].find('>') {
                    let param_value_start = param_name_start + param_name_end + param_value_start_rel + 1;

                    // Find the closing </parameter>
                    if let Some(param_value_end_rel) = invoke_content[param_value_start..].find("</parameter>") {
                        let param_value = &invoke_content[param_value_start..param_value_start + param_value_end_rel];

                        // Try to parse as JSON value, otherwise treat as string
                        let value = if let Ok(json_val) = serde_json::from_str::<Value>(param_value) {
                            json_val
                        } else {
                            Value::String(param_value.to_string())
                        };

                        arguments.insert(param_name.to_string(), value);
                        param_pos = param_value_start + param_value_end_rel + "</parameter>".len();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Generate a unique call_id
    let call_id = format!("minimax_call_{}", uuid::Uuid::new_v4());

    // Convert arguments map to JSON string
    let arguments_str = serde_json::to_string(&arguments).ok()?;

    Some(json!({
        "type": "function_call",
        "name": function_name,
        "arguments": arguments_str,
        "call_id": call_id
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimax_tool_call() {
        let xml = r#"<minimax:tool_call>
<invoke name="apply_patch">
<parameter name="command">*** Begin Patch
*** Add File: /test.md
+# Test
*** End Patch</parameter>
</invoke>
</minimax:tool_call>"#;

        let result = parse_minimax_xml_block(xml);
        assert!(result.is_some());

        let value = result.unwrap();
        assert_eq!(value["type"], "function_call");
        assert_eq!(value["name"], "apply_patch");
        assert!(value["call_id"].as_str().unwrap().starts_with("minimax_call_"));

        // Check arguments can be parsed
        let args_str = value["arguments"].as_str().unwrap();
        let args: Value = serde_json::from_str(args_str).unwrap();
        assert!(args["command"].as_str().unwrap().contains("Test"));
    }

    #[test]
    fn test_parse_multiple_parameters() {
        let xml = r#"<minimax:tool_call>
<invoke name="search_web">
<parameter name="query_tag">["technology"]</parameter>
<parameter name="query_list">["query string"]</parameter>
</invoke>
</minimax:tool_call>"#;

        let result = parse_minimax_xml_block(xml);
        assert!(result.is_some());

        let value = result.unwrap();
        assert_eq!(value["name"], "search_web");

        let args_str = value["arguments"].as_str().unwrap();
        let args: Value = serde_json::from_str(args_str).unwrap();
        assert!(args.get("query_tag").is_some());
        assert!(args.get("query_list").is_some());
    }

    #[test]
    fn test_transform_message_with_tool_call() {
        let mut item = json!({
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "<minimax:tool_call>\n<invoke name=\"test_fn\">\n<parameter name=\"arg1\">value1</parameter>\n</invoke>\n</minimax:tool_call>"
            }]
        });

        let transformed = transform_minimax_tool_calls(&mut item);
        assert!(transformed);
        assert_eq!(item["type"], "function_call");
        assert_eq!(item["name"], "test_fn");
    }
}

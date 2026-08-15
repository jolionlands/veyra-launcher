use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AiToolParam {
    pub(crate) name: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AiToolCall {
    pub(crate) name: String,
    pub(crate) params: Vec<AiToolParam>,
}

pub(crate) fn parse_ai_function_calls(text: &str) -> Vec<AiToolCall> {
    let mut calls = Vec::new();
    let mut offset = 0;
    while let Some(start_rel) = find_ascii_case_insensitive(&text[offset..], "<function") {
        let start = offset + start_rel;
        let Some(open_end_rel) = text[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_rel;
        let tag = &text[start..=open_end];
        let Some(name) = parse_xml_attr(tag, "name").and_then(|value| non_empty(&value)) else {
            offset = open_end + 1;
            continue;
        };

        let body_start = open_end + 1;
        let Some(close_rel) = find_ascii_case_insensitive(&text[body_start..], "</function>")
        else {
            break;
        };
        let body_end = body_start + close_rel;
        let body = &text[body_start..body_end];

        calls.push(AiToolCall {
            name,
            params: parse_ai_function_params(body),
        });
        offset = body_end + "</function>".len();
    }

    calls
}

pub(crate) fn ai_answer_display_text(answer: &str) -> String {
    let without_calls = strip_ai_function_calls(answer);
    let trimmed = without_calls.trim();
    if trimmed.is_empty() && !parse_ai_function_calls(answer).is_empty() {
        return "Suggested action below.".to_string();
    }
    trimmed.to_string()
}

pub(crate) fn normalize_ai_tool_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|ch| match ch {
            '-' | ' ' => '_',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}

pub(crate) fn ai_tool_call_param(call: &AiToolCall, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(param) = call
            .params
            .iter()
            .find(|param| param.name.eq_ignore_ascii_case(name))
        {
            return non_empty(&param.value);
        }
    }

    None
}

pub(crate) fn ai_tool_call(name: &str, param_name: &str, value: &str) -> AiToolCall {
    AiToolCall {
        name: name.to_string(),
        params: vec![AiToolParam {
            name: param_name.to_string(),
            value: value.to_string(),
        }],
    }
}

fn parse_ai_function_params(body: &str) -> Vec<AiToolParam> {
    let mut params = Vec::new();
    let mut offset = 0;
    while let Some(start_rel) = find_ascii_case_insensitive(&body[offset..], "<param") {
        let start = offset + start_rel;
        let Some(open_end_rel) = body[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_rel;
        let tag = &body[start..=open_end];
        let Some(name) = parse_xml_attr(tag, "name").and_then(|value| non_empty(&value)) else {
            offset = open_end + 1;
            continue;
        };

        let value_start = open_end + 1;
        let Some(close_rel) = find_ascii_case_insensitive(&body[value_start..], "</param>") else {
            break;
        };
        let value_end = value_start + close_rel;
        let raw_value = body[value_start..value_end].trim();
        let value = decode_xml_text(strip_cdata(raw_value).trim());

        params.push(AiToolParam { name, value });
        offset = value_end + "</param>".len();
    }

    params
}

fn strip_ai_function_calls(text: &str) -> String {
    let mut output = String::new();
    let mut offset = 0;
    while let Some(start_rel) = find_ascii_case_insensitive(&text[offset..], "<function") {
        let start = offset + start_rel;
        output.push_str(&text[offset..start]);
        let Some(close_rel) = find_ascii_case_insensitive(&text[start..], "</function>") else {
            output.push_str(&text[start..]);
            return output;
        };
        offset = start + close_rel + "</function>".len();
    }
    output.push_str(&text[offset..]);
    output
}

fn parse_xml_attr(tag: &str, attr: &str) -> Option<String> {
    let lowered = tag.to_ascii_lowercase();
    let marker = format!("{}=", attr.to_ascii_lowercase());
    let attr_start = lowered.find(&marker)? + marker.len();
    let quote = tag[attr_start..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_start = attr_start + quote.len_utf8();
    let value_end = tag[value_start..].find(quote)? + value_start;
    Some(decode_xml_text(&tag[value_start..value_end]))
}

fn strip_cdata(value: &str) -> &str {
    value
        .strip_prefix("<![CDATA[")
        .and_then(|inner| inner.strip_suffix("]]>"))
        .unwrap_or(value)
}

fn decode_xml_text(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

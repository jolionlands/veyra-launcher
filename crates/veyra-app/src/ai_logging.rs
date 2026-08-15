use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ai_tools::{AiToolCall, ai_answer_display_text};
use crate::{
    AiConversationMessage, AiEvalReport, AiResponse, AiResponseResult, AiToolSuggestion,
    ai_conversation_role_label, ai_provider_kind_label, category_label, unix_timestamp,
};

const AI_CHAT_LOG_FILE_NAME: &str = "ai-chat-log.jsonl";
const AI_CHAT_SNAPSHOT_DIR_NAME: &str = "ai-chats";

#[derive(Debug, Serialize)]
struct AiChatLogRecord {
    schema_version: u32,
    timestamp: u64,
    session_id: u64,
    turn_index: u32,
    provider_label: String,
    prompt: String,
    request: AiChatLogRequest,
    result: AiChatLogResult,
    tool_suggestions: Vec<AiChatLogToolSuggestion>,
    evaluation: AiEvalReport,
    conversation: Vec<AiConversationMessage>,
}

#[derive(Debug, Serialize)]
struct AiChatLogRequest {
    provider_kind: String,
    model_label: String,
    indexed_tools: usize,
    tool_context_items: usize,
    message_context_items: usize,
    estimated_context_tokens: usize,
    context_limit_tokens: Option<usize>,
    provider_supports_tools: bool,
    native_tool_calls_enabled: bool,
    parsed_tool_calls_enabled: bool,
}

#[derive(Debug, Serialize)]
struct AiChatLogResult {
    kind: String,
    raw_text: Option<String>,
    display_text: Option<String>,
    elapsed_ms: Option<u128>,
}

#[derive(Debug, Serialize)]
struct AiChatLogToolSuggestion {
    call: AiToolCall,
    label: String,
    detail: String,
    matched: bool,
    matched_item_id: Option<String>,
    matched_item_label: Option<String>,
    matched_category: Option<String>,
}

pub(super) fn append_ai_chat_log(
    profile_dir: &Path,
    response: &AiResponse,
    conversation: &[AiConversationMessage],
    evaluation: &AiEvalReport,
) -> io::Result<PathBuf> {
    fs::create_dir_all(profile_dir)?;
    let path = ai_chat_log_path(profile_dir);
    let record = ai_chat_log_record(response, conversation, evaluation);
    let raw = serde_json::to_string(&record).map_err(io::Error::other)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{raw}")?;
    Ok(path)
}

pub(super) fn ensure_ai_chat_log_file(profile_dir: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(profile_dir)?;
    let path = ai_chat_log_path(profile_dir);
    if !path.exists() {
        fs::write(&path, "")?;
    }
    Ok(path)
}

pub(super) fn save_ai_chat_snapshot(
    profile_dir: &Path,
    session_id: u64,
    conversation: &[AiConversationMessage],
    response: Option<&AiResponse>,
    evaluation: Option<&AiEvalReport>,
) -> io::Result<PathBuf> {
    let dir = ai_chat_snapshot_dir(profile_dir);
    fs::create_dir_all(&dir)?;
    let timestamp = unix_timestamp();
    let path = dir.join(format!("veyra-ai-session-{session_id}-{timestamp}.md"));
    let mut text = String::new();
    text.push_str("# Veyra AI Chat\n\n");
    text.push_str(&format!("- Session: `{session_id}`\n"));
    text.push_str(&format!("- Saved: `{timestamp}` Unix seconds\n"));
    if let Some(response) = response {
        text.push_str(&format!("- Provider: `{}`\n", response.provider_label));
        text.push_str(&format!("- Prompt: `{}`\n", response.prompt.trim()));
    }
    if let Some(evaluation) = evaluation {
        text.push_str(&format!("- Eval: `{}`\n", evaluation.summary));
    }
    text.push('\n');

    if let Some(evaluation) = evaluation {
        text.push_str("## Eval Checks\n\n");
        for check in &evaluation.checks {
            let status = if check.passed { "PASS" } else { "FAIL" };
            text.push_str(&format!(
                "- `{status}` `{}`: {}\n",
                check.name, check.detail
            ));
        }
        text.push('\n');
    }

    text.push_str("## Conversation\n\n");
    if conversation.is_empty() {
        text.push_str("_No visible conversation messages._\n\n");
    } else {
        for message in conversation {
            text.push_str(&format!(
                "### {}\n\n{}\n\n",
                ai_conversation_role_label(message.role),
                message.text.trim()
            ));
        }
    }

    if let Some(response) = response
        && !response.tool_suggestions.is_empty()
    {
        text.push_str("## Tool Suggestions\n\n");
        for suggestion in &response.tool_suggestions {
            let status = if suggestion.result.is_some() {
                "resolved"
            } else {
                "unresolved"
            };
            text.push_str(&format!(
                "- `{}` `{}`: {}\n",
                status, suggestion.label, suggestion.detail
            ));
        }
    }

    fs::write(&path, text)?;
    Ok(path)
}

pub(super) fn ai_chat_log_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(AI_CHAT_LOG_FILE_NAME)
}

pub(super) fn ai_chat_snapshot_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join(AI_CHAT_SNAPSHOT_DIR_NAME)
}

fn ai_chat_log_record(
    response: &AiResponse,
    conversation: &[AiConversationMessage],
    evaluation: &AiEvalReport,
) -> AiChatLogRecord {
    let (kind, raw_text, display_text) = match &response.result {
        AiResponseResult::Pending => ("pending".to_string(), None, None),
        AiResponseResult::Answer(answer) => (
            "answer".to_string(),
            Some(answer.clone()),
            Some(ai_answer_display_text(answer)),
        ),
        AiResponseResult::Error(error) => (
            "error".to_string(),
            Some(error.clone()),
            Some(error.clone()),
        ),
    };

    AiChatLogRecord {
        schema_version: 1,
        timestamp: unix_timestamp(),
        session_id: response.session_id,
        turn_index: response.turn_index,
        provider_label: response.provider_label.clone(),
        prompt: response.prompt.clone(),
        request: AiChatLogRequest {
            provider_kind: ai_provider_kind_label(response.request.provider_kind).to_string(),
            model_label: response.request.model_label.clone(),
            indexed_tools: response.request.indexed_tools,
            tool_context_items: response.request.tool_context_items,
            message_context_items: response.request.message_context_items,
            estimated_context_tokens: response.request.estimated_context_tokens,
            context_limit_tokens: response.request.context_limit_tokens,
            provider_supports_tools: response.request.provider_supports_tools,
            native_tool_calls_enabled: response.request.native_tool_calls_enabled,
            parsed_tool_calls_enabled: response.request.parsed_tool_calls_enabled,
        },
        result: AiChatLogResult {
            kind,
            raw_text,
            display_text,
            elapsed_ms: response.elapsed_ms,
        },
        tool_suggestions: response
            .tool_suggestions
            .iter()
            .map(ai_chat_log_tool_suggestion)
            .collect(),
        evaluation: evaluation.clone(),
        conversation: conversation.to_vec(),
    }
}

fn ai_chat_log_tool_suggestion(suggestion: &AiToolSuggestion) -> AiChatLogToolSuggestion {
    let matched_item = suggestion.result.as_ref().map(|result| &result.item);
    AiChatLogToolSuggestion {
        call: suggestion.call.clone(),
        label: suggestion.label.clone(),
        detail: suggestion.detail.clone(),
        matched: matched_item.is_some(),
        matched_item_id: matched_item.map(|item| item.id.clone()),
        matched_item_label: matched_item.map(|item| item.label.clone()),
        matched_category: matched_item.map(|item| category_label(item).to_string()),
    }
}

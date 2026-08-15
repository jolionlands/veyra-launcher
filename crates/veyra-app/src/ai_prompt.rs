use veyra_core::CatalogItem;

use crate::{
    AiConversationMessage, AiConversationRole, category_label, current_local_datetime_parts,
    format_clock_time, truncate_for_label,
};

const AI_COMPACT_HISTORY_CHARS: usize = 160;
const AI_COMPACT_TOOL_LABEL_CHARS: usize = 48;

#[derive(Debug, Clone)]
pub(crate) struct AiPromptPlan {
    pub(crate) prompt: String,
    pub(crate) tool_context_items: usize,
    pub(crate) message_context_items: usize,
    pub(crate) estimated_provider_tokens: usize,
}

impl AiPromptPlan {
    pub(crate) fn new(
        prompt: String,
        tool_context_items: usize,
        message_context_items: usize,
        estimated_provider_tokens: usize,
    ) -> Self {
        Self {
            prompt,
            tool_context_items,
            message_context_items,
            estimated_provider_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AiContextItem {
    label: String,
    category: &'static str,
    subtitle: Option<String>,
}

impl AiContextItem {
    pub(crate) fn from_catalog_item(item: &CatalogItem) -> Self {
        Self {
            label: item.label.clone(),
            category: category_label(item),
            subtitle: item.subtitle.clone(),
        }
    }
}

pub(crate) fn format_ai_model_prompt(
    prompt: &str,
    tool_context: &[AiContextItem],
    clipboard_context: Option<&str>,
    conversation: &[AiConversationMessage],
    message_limit: usize,
    compact: bool,
) -> String {
    let mut text = String::new();
    let local_time = current_local_datetime_parts();
    if compact {
        text.push_str(&format!(
            "Now: {}, {} {}, {} {} local.\n\n",
            local_time.weekday_name(),
            local_time.month_name(),
            local_time.day,
            local_time.year,
            format_clock_time(&local_time)
        ));
    } else {
        text.push_str(&format!(
            "Live system context: local date/time is {}, {} {}, {} at {}. Use this for current-date/time reasoning; do not infer current time from model training data.\n\n",
            local_time.weekday_name(),
            local_time.month_name(),
            local_time.day,
            local_time.year,
            format_clock_time(&local_time)
        ));
    }

    let history = conversation
        .iter()
        .rev()
        .take(message_limit)
        .collect::<Vec<_>>();
    if !history.is_empty() {
        text.push_str(if compact {
            "Recent:\n"
        } else {
            "Recent conversation context:\n"
        });
        for message in history.iter().rev() {
            text.push_str(match message.role {
                AiConversationRole::User => "User: ",
                AiConversationRole::Assistant => "Assistant: ",
                AiConversationRole::System => "System: ",
            });
            if compact {
                text.push_str(&truncate_for_label(
                    message.text.trim(),
                    AI_COMPACT_HISTORY_CHARS,
                ));
            } else {
                text.push_str(message.text.trim());
            }
            text.push('\n');
        }
        text.push('\n');
    }

    text.push_str(if compact {
        "Answer the latest user request only. Use recent context only when the latest request refers to it.\n\n"
    } else {
        "The current user request below has priority. Use recent conversation only when the current request clearly refers to it; do not repeat an earlier time, timezone, or location answer for a different request.\n\n"
    });

    if let Some(clipboard_context) = clipboard_context {
        if compact {
            text.push_str("Captured clipboard text:\n");
            text.push_str(&truncate_for_label(
                clipboard_context,
                AI_COMPACT_HISTORY_CHARS * 2,
            ));
            text.push_str("\n\n");
        } else {
            text.push_str("Captured clipboard text, likely the user's selected or copied input:\n");
            text.push_str(clipboard_context.trim());
            text.push_str("\n\n");
        }
    }

    if !tool_context.is_empty() {
        if compact {
            text.push_str("Veyra context:\n");
            for (index, item) in tool_context.iter().enumerate() {
                text.push_str(&format!(
                    "{}. {} {}\n",
                    index + 1,
                    item.category,
                    truncate_for_label(&item.label, AI_COMPACT_TOOL_LABEL_CHARS)
                ));
            }
            text.push_str("To ask Veyra to act, append one XML call and use real values, not placeholders. Example: <function name=\"open_result\"><param name=\"query\">WireGuard</param></function>. Also allowed: search(query), open_url(url), copy_to_clipboard(text), calculate(expression), current_time(location). Otherwise answer normally. Veyra confirms before running.\n");
        } else {
            text.push_str("Relevant Veyra launcher/tool context:\n");
            for (index, item) in tool_context.iter().enumerate() {
                text.push_str(&format!(
                    "{}. [{}] {}",
                    index + 1,
                    item.category,
                    item.label
                ));
                if let Some(subtitle) = &item.subtitle {
                    text.push_str(" - ");
                    text.push_str(subtitle);
                }
                text.push('\n');
            }
            text.push_str("\nIf the user wants Veyra to run, open, search, or copy something, you may request exactly one action by appending one XML function call. Veyra will show it for confirmation before anything runs. Do not claim the action already happened.\n");
            text.push_str("Allowed function calls:\n");
            text.push_str("<function name=\"open_result\"><param name=\"query\">ACTUAL_ITEM_OR_SEARCH_TEXT</param></function>\n");
            text.push_str("<function name=\"search\"><param name=\"query\">ACTUAL_WEB_SEARCH_TEXT</param></function>\n");
            text.push_str("<function name=\"open_url\"><param name=\"url\">https://example.com</param></function>\n");
            text.push_str("<function name=\"copy_to_clipboard\"><param name=\"text\">ACTUAL_TEXT_TO_COPY</param></function>\n");
            text.push_str("<function name=\"calculate\"><param name=\"expression\">2 + 2</param></function>\n");
            text.push_str("<function name=\"current_time\"><param name=\"location\">Tokyo</param></function>\n");
            text.push_str("Use actual values from the request or context, never placeholder names. Example: <function name=\"open_result\"><param name=\"query\">WireGuard</param></function>\n");
            text.push_str(
                "For normal questions, answer normally and do not include a function call.\n",
            );
        }
        text.push('\n');
    }

    text.push_str(if compact {
        "User:\n"
    } else {
        "Current user request:\n"
    });
    text.push_str(prompt.trim());
    text
}

pub(crate) fn prompt_needs_conversation_context(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    if lowered.contains("previous")
        || lowered.contains("earlier")
        || lowered.contains("above")
        || lowered.contains("you said")
        || lowered.contains("last answer")
        || lowered.contains("same")
        || lowered.contains("again")
        || lowered.contains("continue")
    {
        return true;
    }

    let words = prompt_words(&lowered);
    let has = |needle: &str| words.contains(&needle);
    has("it") || has("that") || has("this") || has("those") || has("them") || has("one")
}

fn prompt_words(value: &str) -> Vec<&str> {
    value
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .collect()
}

use std::cmp::Reverse;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
#[cfg(test)]
use veyra_core::SearchResult;
use veyra_core::{CatalogItem, ItemCategory};

use crate::unix_timestamp;

const DEFAULT_HISTORY_LIMIT: usize = 5_000;
const HISTORY_FILE_NAME: &str = "history.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LaunchHistory {
    pub(crate) entries: Vec<LaunchHistoryEntry>,
}

impl LaunchHistory {
    pub(crate) fn record(&mut self, item: &CatalogItem, query: &str) {
        let now = unix_timestamp();
        let normalized_query = normalize_history_text(query);

        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.item_id == item.id)
        {
            entry.label = item.label.clone();
            entry.source = item.source.clone();
            entry.launch_count = entry.launch_count.saturating_add(1).max(1);
            entry.last_query = normalized_query;
            entry.last_used_unix = now;
        } else {
            self.entries.push(LaunchHistoryEntry {
                item_id: item.id.clone(),
                label: item.label.clone(),
                source: item.source.clone(),
                launch_count: 1,
                last_query: normalized_query,
                last_used_unix: now,
            });
        }

        self.entries
            .sort_by_key(|entry| Reverse(entry.last_used_unix));
        self.entries.truncate(DEFAULT_HISTORY_LIMIT);
    }

    pub(crate) fn boost_for(&self, item: &CatalogItem, query: &str) -> i32 {
        let Some(entry) = self.entries.iter().find(|entry| entry.item_id == item.id) else {
            return 0;
        };

        let launches = entry.launch_count.min(20) as i32;
        let mut boost = 120 + launches * 35;
        let query = normalize_history_text(query);
        if !query.is_empty() && entry.last_query == query {
            boost += 120;
        }
        if matches!(
            item.category,
            ItemCategory::App | ItemCategory::Command | ItemCategory::Tool
        ) {
            boost += 40;
        }

        boost.min(900)
    }

    pub(crate) fn total_launches(&self) -> u32 {
        self.entries.iter().map(|entry| entry.launch_count).sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LaunchHistoryEntry {
    pub(crate) item_id: String,
    pub(crate) label: String,
    pub(crate) source: String,
    pub(crate) launch_count: u32,
    pub(crate) last_query: String,
    pub(crate) last_used_unix: u64,
}

#[cfg(test)]
pub(crate) fn recent_launch_results_from(
    catalog: &[CatalogItem],
    history: &LaunchHistory,
    limit: usize,
) -> Vec<SearchResult> {
    history
        .entries
        .iter()
        .take(limit)
        .enumerate()
        .filter_map(|(index, entry)| {
            let mut item = catalog
                .iter()
                .find(|item| item.id == entry.item_id)
                .cloned()?;
            let subtitle = item
                .subtitle
                .as_deref()
                .map(|subtitle| format!("Recent - {subtitle}"))
                .unwrap_or_else(|| "Recent".to_string());
            item.subtitle = Some(subtitle);
            Some(SearchResult {
                item,
                score: 3_000 - index as i32,
            })
        })
        .collect()
}

pub(crate) fn load_launch_history(profile_dir: &Path) -> LaunchHistory {
    let path = history_path(profile_dir);
    let Ok(raw) = fs::read_to_string(path) else {
        return LaunchHistory::default();
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

pub(crate) fn save_launch_history(profile_dir: &Path, history: &LaunchHistory) -> io::Result<()> {
    fs::create_dir_all(profile_dir)?;
    let raw = serde_json::to_string_pretty(history).map_err(io::Error::other)?;
    fs::write(history_path(profile_dir), raw)
}

pub(crate) fn history_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(HISTORY_FILE_NAME)
}

fn normalize_history_text(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

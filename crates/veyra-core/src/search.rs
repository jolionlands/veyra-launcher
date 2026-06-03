use crate::CatalogItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub item: CatalogItem,
    pub score: i32,
}

pub fn search(items: &[CatalogItem], query: &str) -> Vec<SearchResult> {
    let normalized = normalize(query);
    let mut results: Vec<_> = items
        .iter()
        .filter_map(|item| {
            score_item(item, &normalized).map(|score| SearchResult {
                item: item.clone(),
                score,
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.item.label.cmp(&b.item.label))
    });
    results
}

fn score_item(item: &CatalogItem, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(item.score_boost);
    }

    let label = normalize(&item.label);
    let subtitle = item.subtitle.as_deref().map(normalize).unwrap_or_default();
    let keywords = item.keywords.iter().map(|keyword| normalize(keyword));

    let mut best = score_text(&label, query);
    best = best.max(score_text(&subtitle, query) - 100);

    for keyword in keywords {
        best = best.max(score_text(&keyword, query) + 75);
    }

    if best <= 0 {
        None
    } else {
        Some(best + item.score_boost)
    }
}

fn score_text(text: &str, query: &str) -> i32 {
    if text == query {
        return 1000;
    }
    if text.starts_with(query) {
        return 850 - text.len() as i32;
    }
    if text.contains(query) {
        return 650 - text.find(query).unwrap_or_default() as i32;
    }
    if acronym(text).starts_with(query) {
        return 575 - text.len() as i32;
    }
    if ordered_match(text, query) {
        return 400 - text.len() as i32;
    }
    0
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn acronym(text: &str) -> String {
    text.split_whitespace()
        .filter_map(|word| word.chars().next())
        .collect()
}

fn ordered_match(text: &str, query: &str) -> bool {
    let mut chars = text.chars();
    query.chars().all(|needle| chars.any(|ch| ch == needle))
}

#[cfg(test)]
mod tests {
    use crate::{search, seed_catalog};

    #[test]
    fn ranks_display_settings_for_resolution_query() {
        let results = search(&seed_catalog(), "resolution");
        assert_eq!(results.first().unwrap().item.id, "settings.display");
    }

    #[test]
    fn finds_github_from_alias_keyword() {
        let results = search(&seed_catalog(), "gh");
        assert_eq!(results.first().unwrap().item.id, "web.github");
    }

    #[test]
    fn returns_seed_items_for_empty_query() {
        let results = search(&seed_catalog(), "");
        assert!(!results.is_empty());
    }
}

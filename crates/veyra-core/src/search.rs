use crate::CatalogItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub item: CatalogItem,
    pub score: i32,
}

pub fn search(items: &[CatalogItem], query: &str) -> Vec<SearchResult> {
    SearchIndex::new(items).search(items, query)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchIndex {
    items: Vec<IndexedCatalogItem>,
}

impl SearchIndex {
    pub fn new(items: &[CatalogItem]) -> Self {
        Self {
            items: items.iter().map(IndexedCatalogItem::from).collect(),
        }
    }

    pub fn search(&self, catalog: &[CatalogItem], query: &str) -> Vec<SearchResult> {
        if self.items.len() != catalog.len() {
            return search(catalog, query);
        }

        let normalized = normalize(query);
        let mut results: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                score_indexed_item(item, &normalized).map(|score| SearchResult {
                    item: catalog[index].clone(),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedCatalogItem {
    label: String,
    subtitle: String,
    keywords: Vec<String>,
    score_boost: i32,
}

impl From<&CatalogItem> for IndexedCatalogItem {
    fn from(item: &CatalogItem) -> Self {
        Self {
            label: normalize(&item.label),
            subtitle: item.subtitle.as_deref().map(normalize).unwrap_or_default(),
            keywords: item
                .keywords
                .iter()
                .map(|keyword| normalize(keyword))
                .collect(),
            score_boost: item.score_boost,
        }
    }
}

fn score_indexed_item(item: &IndexedCatalogItem, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(item.score_boost);
    }

    let label_score = score_text(&item.label, query);
    let subtitle_score = score_text_literal(&item.subtitle, query).saturating_sub(100);
    let mut best = label_score.max(subtitle_score);

    for keyword in &item.keywords {
        let keyword_score = score_text(keyword, query);
        if keyword_score > 0 {
            best = best.max(keyword_score + 75);
        }
    }

    if best <= 0 {
        None
    } else {
        Some(best + item.score_boost)
    }
}

pub fn search_without_index(items: &[CatalogItem], query: &str) -> Vec<SearchResult> {
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

    let label_score = score_text(&label, query);
    let subtitle_score = score_text_literal(&subtitle, query).saturating_sub(100);
    let mut best = label_score.max(subtitle_score);

    for keyword in keywords {
        let keyword_score = score_text(&keyword, query);
        if keyword_score > 0 {
            best = best.max(keyword_score + 75);
        }
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
        return (850 - text.len() as i32).max(50);
    }
    if let Some(position) = text.find(query) {
        return (650 - position as i32).max(40);
    }
    if acronym(text).starts_with(query) {
        return (575 - text.len() as i32).max(30);
    }
    if ordered_match(text, query) {
        return (400 - text.len() as i32).max(20);
    }
    0
}

fn score_text_literal(text: &str, query: &str) -> i32 {
    if text == query {
        return 1000;
    }
    if text.starts_with(query) {
        return (850 - text.len() as i32).max(50);
    }
    if let Some(position) = text.find(query) {
        return (650 - position as i32).max(40);
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

    #[test]
    fn indexed_search_matches_unindexed_ranking() {
        let catalog = seed_catalog();
        let indexed = super::SearchIndex::new(&catalog).search(&catalog, "settings");
        let unindexed = super::search_without_index(&catalog, "settings");

        assert_eq!(
            indexed
                .iter()
                .map(|result| (&result.item.id, result.score))
                .collect::<Vec<_>>(),
            unindexed
                .iter()
                .map(|result| (&result.item.id, result.score))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn score_text_never_returns_negative_for_a_match() {
        let long_text = format!("{} query here", "a".repeat(2000));
        assert_eq!(super::score_text(&long_text, "query"), 40);

        assert!(
            super::score_text("find query here", "query") > 0,
            "contained query should score positively"
        );
        assert!(
            super::score_text("query starts here", "query") > 0,
            "prefix query should score positively"
        );
    }

    #[test]
    fn long_subtitle_does_not_hide_keyword_match() {
        use crate::{CatalogItem, ItemCategory};

        let long_subtitle = format!(
            "{}/very/deep/path/to/some/application/binary.exe",
            "C:".repeat(400)
        );
        let item = CatalogItem::new("app", "My App", ItemCategory::App, "test")
            .subtitle(long_subtitle)
            .keywords(["myapp"]);

        let results = search(&[item], "myapp");
        assert!(
            !results.is_empty(),
            "a keyword match should still surface even with a long subtitle"
        );
    }

    #[test]
    fn unrelated_keywords_do_not_boost_results() {
        use crate::{CatalogItem, ItemCategory};

        let item = CatalogItem::new("app", "My App", ItemCategory::App, "test")
            .subtitle("Some path")
            .keywords(["unrelated"]);

        let results = search(&[item], "binary");
        assert!(
            results.is_empty(),
            "unrelated keywords should not make an item match"
        );
    }
}

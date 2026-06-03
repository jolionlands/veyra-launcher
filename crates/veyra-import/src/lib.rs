use std::collections::HashMap;

use veyra_core::config::{CommandEntry, WebSearchEntry};

#[derive(Debug)]
struct ParsedSection {
    kind: SectionKind,
    name: String,
    properties: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy)]
enum SectionKind {
    Command,
    WebSearch,
    Ignore,
}

impl ParsedSection {
    fn get(&self, key: &str) -> Option<&str> {
        self.properties
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Parses Keypirinha-style custom command sections (`[cmd/<name>]`) from INI input.
///
/// Unknown sections and malformed lines are ignored.
pub fn parse_apps_ini(input: impl AsRef<str>) -> Vec<CommandEntry> {
    let sections = parse_ini_sections(input.as_ref());

    sections
        .into_iter()
        .filter_map(|section| {
            if !matches!(section.kind, SectionKind::Command) {
                return None;
            }

            section_to_command(section)
        })
        .collect()
}

/// Parses Keypirinha-style web search sections (`[site/<alias>]`) from INI input.
///
/// Unknown sections and malformed lines are ignored.
pub fn parse_websearch_ini(input: impl AsRef<str>) -> Vec<WebSearchEntry> {
    let sections = parse_ini_sections(input.as_ref());

    sections
        .into_iter()
        .filter_map(|section| {
            if !matches!(section.kind, SectionKind::WebSearch) {
                return None;
            }

            section_to_web_search(section)
        })
        .collect()
}

fn parse_ini_sections(input: &str) -> Vec<ParsedSection> {
    let mut sections = Vec::new();
    let mut current: Option<ParsedSection> = None;

    for line in input.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if let Some((kind, name)) = parse_section_header(trimmed) {
            if let Some(section) = current.take() {
                sections.push(section);
            }

            current = Some(ParsedSection {
                kind,
                name,
                properties: HashMap::new(),
            });
            continue;
        }

        if let Some((key, value)) = parse_kv_line(trimmed) {
            if let Some(section) = &mut current {
                section.properties.insert(key, value);
            }
            continue;
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }

    sections
}

fn parse_section_header(trimmed: &str) -> Option<(SectionKind, String)> {
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let inner = trimmed.trim_start_matches('[').trim_end_matches(']');
    let (kind, name) = inner.split_once('/')?;
    let kind = match kind.to_ascii_lowercase().as_str() {
        "cmd" => SectionKind::Command,
        "site" => SectionKind::WebSearch,
        _ => SectionKind::Ignore,
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        return None;
    }

    Some((kind, name))
}

fn parse_kv_line(trimmed: &str) -> Option<(String, String)> {
    let mut parts = trimmed.splitn(2, '=');
    let key = parts.next()?.trim();
    let value = parts.next()?.trim();

    if key.is_empty() {
        return None;
    }
    if key.starts_with('#') || key.starts_with(';') {
        return None;
    }

    Some((key.to_ascii_lowercase(), value.to_string()))
}

fn section_to_command(section: ParsedSection) -> Option<CommandEntry> {
    let id = section.name.clone();
    let command = section
        .get("command")
        .or_else(|| section.get("cmd"))
        .or_else(|| section.get("path"))
        .or_else(|| section.get("exe"))
        .or_else(|| section.get("target"))?;

    let label = section
        .get("item_label")
        .or_else(|| section.get("label"))
        .or_else(|| section.get("name"))
        .unwrap_or(&section.name);

    let args = section
        .get("args")
        .or_else(|| section.get("parameters"))
        .or_else(|| section.get("default_params"))
        .map(split_command_args)
        .unwrap_or_default();

    let terminal = section
        .get("terminal")
        .or_else(|| section.get("auto_terminal"))
        .or_else(|| section.get("run_in_terminal"))
        .and_then(parse_bool)
        .unwrap_or(false);

    let requires_confirmation = section
        .get("requires_confirmation")
        .or_else(|| section.get("confirm"))
        .or_else(|| section.get("ask"))
        .and_then(parse_bool)
        .unwrap_or(false);

    let keywords = section
        .get("keywords")
        .map(parse_keywords)
        .unwrap_or_default();

    Some(CommandEntry {
        id,
        label: label.to_string(),
        command: command.to_string(),
        args,
        terminal,
        requires_confirmation,
        keywords,
    })
}

fn section_to_web_search(section: ParsedSection) -> Option<WebSearchEntry> {
    let mut url = section
        .get("url")
        .or_else(|| section.get("search_url"))
        .or_else(|| section.get("query_url"))?
        .to_string();

    let alias = section.name.clone();
    let label = section
        .get("label")
        .or_else(|| section.get("name"))
        .unwrap_or(&alias)
        .to_string();

    if !url.contains("{query}") {
        url = url
            .replace("{searchterms}", "{query}")
            .replace("{searchTerms}", "{query}");
        if url.contains("%s") {
            url = url.replace("%s", "{query}");
        } else if url.contains("{}") {
            url = url.replace("{}", "{query}");
        }
    }
    let id = alias.clone();

    Some(WebSearchEntry {
        id,
        alias,
        label,
        url,
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Some(true),
        "0" | "false" | "no" | "off" | "disabled" => Some(false),
        _ => None,
    }
}

fn parse_keywords(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn split_command_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in input.chars() {
        match ch {
            '"' | '\'' => {
                if in_quotes {
                    if ch == quote_char {
                        in_quotes = false;
                    } else {
                        current.push(ch);
                    }
                } else {
                    in_quotes = true;
                    quote_char = ch;
                }
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_sections_with_defaults_and_aliases() {
        let input = r#"
            ; App commands from Keypirinha-style INI
            [cmd/Display Settings]
            item_label = System Display
            command = explorer.exe
            args = --open "folder with spaces"
            auto_terminal = no
            keywords = display,settings

            [cmd/Utilities\Repair]
            name = Utility Repair
            cmd = repair-tool
            parameters = -f --safe
            terminal = yes
            ; unknown section type should be ignored
            [catalog/Path]
            path = ignored-placeholder

            [cmd/NeedsCommand]
            label = No Command
        "#;

        let mut commands = parse_apps_ini(input);

        assert_eq!(commands.len(), 2);

        let first = commands.remove(0);
        assert_eq!(first.id, "Display Settings");
        assert_eq!(first.label, "System Display");
        assert_eq!(first.command, "explorer.exe");
        assert_eq!(first.args, vec!["--open", "folder with spaces"]);
        assert!(!first.terminal);
        assert_eq!(first.keywords, vec!["display", "settings"]);

        let second = commands.remove(0);
        assert_eq!(second.id, "Utilities\\Repair");
        assert_eq!(second.label, "Utility Repair");
        assert_eq!(second.command, "repair-tool");
        assert_eq!(second.args, vec!["-f", "--safe"]);
        assert!(second.terminal);
        assert!(!second.requires_confirmation);
    }

    #[test]
    fn ignores_command_with_missing_executable() {
        let input = r#"
            [cmd/OnlyLabel]
            label = Has No Command
            keywords = skip
        "#;

        assert!(parse_apps_ini(input).is_empty());
    }

    #[test]
    fn parses_web_search_sections_and_keeps_query_template() {
        let input = r#"
            [site/gh]
            label = GitHub Code
            search_url = https://github.com/search?q=%s&type=code

            [site/ddg]
            name = DuckDuckGo
            url = https://duckduckgo.com/?q={searchTerms}&ia=web

            [app/ignore]
            label = Not a site
        "#;

        let sites = parse_websearch_ini(input);
        assert_eq!(sites.len(), 2);

        assert_eq!(sites[0].id, "gh");
        assert_eq!(sites[0].alias, "gh");
        assert_eq!(sites[0].label, "GitHub Code");
        assert_eq!(
            sites[0].url,
            "https://github.com/search?q={query}&type=code"
        );

        assert_eq!(sites[1].id, "ddg");
        assert_eq!(sites[1].label, "DuckDuckGo");
        assert_eq!(sites[1].url, "https://duckduckgo.com/?q={query}&ia=web");
    }
}

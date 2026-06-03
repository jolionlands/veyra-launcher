use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use veyra_core::config::{CommandEntry, WebSearchEntry};

const APPS_INI_CANDIDATES: [&str; 4] = [
    "Profile/User/Apps.ini",
    "Profile/User/apps.ini",
    "Apps.ini",
    "apps.ini",
];
const WEB_SEARCH_INI_CANDIDATES: [&str; 4] = [
    "Profile/User/WebSearch.ini",
    "Profile/User/websearch.ini",
    "WebSearch.ini",
    "websearch.ini",
];

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ImportedProfile {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub web_search: Vec<WebSearchEntry>,
}

impl ImportedProfile {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.web_search.is_empty()
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

#[derive(Debug)]
pub enum ImportError {
    SourceNotFound(PathBuf),
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::SourceNotFound(path) => {
                write!(
                    formatter,
                    "source profile does not exist: {}",
                    path.display()
                )
            }
            ImportError::ReadFile { path, source } => {
                write!(formatter, "could not read {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImportError::SourceNotFound(_) => None,
            ImportError::ReadFile { source, .. } => Some(source),
        }
    }
}

pub fn import_keypirinha_profile(
    source_root: impl AsRef<Path>,
) -> Result<ImportedProfile, ImportError> {
    let source_root = source_root.as_ref();
    if !source_root.exists() {
        return Err(ImportError::SourceNotFound(source_root.to_path_buf()));
    }

    let commands = read_first_existing(source_root, &APPS_INI_CANDIDATES)?
        .map(parse_apps_ini)
        .unwrap_or_default();
    let web_search = read_first_existing(source_root, &WEB_SEARCH_INI_CANDIDATES)?
        .map(parse_websearch_ini)
        .unwrap_or_default();

    Ok(ImportedProfile {
        commands,
        web_search,
    })
}

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

fn read_first_existing(
    source_root: &Path,
    candidates: &[&str],
) -> Result<Option<String>, ImportError> {
    for candidate in candidates {
        let path = source_root.join(candidate);
        if !path.exists() {
            continue;
        }

        return std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|source| ImportError::ReadFile { path, source });
    }

    Ok(None)
}

fn parse_ini_sections(input: &str) -> Vec<ParsedSection> {
    let mut sections = Vec::new();
    let mut current: Option<ParsedSection> = None;

    for line in input.lines() {
        let trimmed = line.trim_start_matches('\u{feff}').trim();

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
    use veyra_core::config::VeyraConfig;

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
    fn parses_bom_prefixed_ini_files() {
        let input = "\u{feff}[cmd/Display]\nlabel = Display Settings\ncommand = explorer.exe\n";

        let commands = parse_apps_ini(input);

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].id, "Display");
        assert_eq!(commands[0].command, "explorer.exe");
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

    #[test]
    fn imports_keypirinha_profile_from_standard_layout() {
        let root = temp_source_dir();
        let profile = root.join("Profile").join("User");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join("Apps.ini"),
            r#"
                [cmd/Display]
                item_label = Display Settings
                command = explorer.exe
                args = ms-settings:display
            "#,
        )
        .unwrap();
        std::fs::write(
            profile.join("WebSearch.ini"),
            r#"
                [site/gh]
                label = GitHub
                url = https://github.com/search?q=%s
            "#,
        )
        .unwrap();

        let imported = import_keypirinha_profile(&root).unwrap();
        assert_eq!(imported.commands.len(), 1);
        assert_eq!(imported.web_search.len(), 1);

        let toml = imported.to_toml_string().unwrap();
        let config = VeyraConfig::from_toml_str(&toml).unwrap();
        assert_eq!(config.commands[0].label, "Display Settings");
        assert_eq!(config.web_search[0].alias, "gh");

        std::fs::remove_dir_all(root).ok();
    }

    fn temp_source_dir() -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("veyra-import-test-{nanos}"));
        path
    }
}

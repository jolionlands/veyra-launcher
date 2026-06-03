use std::collections::VecDeque;
use std::process::Command;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use veyra_core::config::CatalogProfile;
use veyra_core::{Action, ActionKind, CatalogItem, ItemCategory};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const WINDOWS_DEFAULT_PATHEXT: [&str; 11] = [
    ".COM", ".EXE", ".BAT", ".CMD", ".VBS", ".VBE", ".JS", ".JSE", ".WSF", ".WSH", ".MSC",
];

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Macos,
    Other,
}

pub fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Other
    }
}

pub fn profile_dir(app_name: &str) -> PathBuf {
    if let Some(portable) = portable_profile_dir() {
        return portable;
    }

    match current_platform() {
        Platform::Windows => std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(app_name),
        Platform::Linux | Platform::Macos => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(app_name.to_ascii_lowercase()),
        Platform::Other => PathBuf::from(".").join(app_name),
    }
}

pub fn discover_platform_catalog_items() -> Vec<CatalogItem> {
    let mut seen_by_path = HashSet::new();
    let mut seen_by_name = HashSet::new();
    let mut items = discover_path_executables_with_seen(&mut seen_by_name, &mut seen_by_path);

    items.extend(discover_start_menu_shortcuts(
        &mut seen_by_name,
        &mut seen_by_path,
    ));

    items
}

pub fn discover_path_executables() -> Vec<CatalogItem> {
    let mut seen_by_path = HashSet::new();
    let mut seen_by_name = HashSet::new();

    discover_path_executables_with_seen(&mut seen_by_name, &mut seen_by_path)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileCatalogDiscovery {
    pub items: Vec<CatalogItem>,
    pub enabled_profiles: usize,
    pub skipped_profiles: usize,
    pub skipped_paths: usize,
}

pub fn discover_file_catalog_items(profiles: &[CatalogProfile]) -> FileCatalogDiscovery {
    let mut discovery = FileCatalogDiscovery::default();
    let mut seen_paths = HashSet::new();

    for profile in profiles {
        if !profile.enabled {
            discovery.skipped_profiles += 1;
            continue;
        }
        discovery.enabled_profiles += 1;

        for raw_path in &profile.paths {
            let path = expand_path(raw_path);
            if !path.exists() {
                discovery.skipped_paths += 1;
                continue;
            }

            if path.is_file() {
                maybe_add_catalog_path(&path, profile, &mut seen_paths, &mut discovery.items);
                continue;
            }

            if !path.is_dir() {
                discovery.skipped_paths += 1;
                continue;
            }

            scan_catalog_dir(&path, profile, &mut seen_paths, &mut discovery.items);
        }
    }

    discovery
}

fn scan_catalog_dir(
    root: &Path,
    profile: &CatalogProfile,
    seen_paths: &mut HashSet<String>,
    items: &mut Vec<CatalogItem>,
) {
    let mut queue = VecDeque::from([(root.to_path_buf(), 0_u32)]);
    let mut visited_dirs = HashSet::new();

    while let Some((dir, depth)) = queue.pop_front() {
        let dir_key = normalize_path_key(&dir);
        if !visited_dirs.insert(dir_key) {
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = if profile.follow_symlinks {
                fs::metadata(&path)
            } else {
                fs::symlink_metadata(&path)
            };
            let Ok(metadata) = metadata else {
                continue;
            };

            if metadata.is_dir() {
                maybe_add_catalog_path(&path, profile, seen_paths, items);
                if profile.recursive && profile.max_depth.is_none_or(|max_depth| depth < max_depth)
                {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }

            if metadata.is_file() {
                maybe_add_catalog_path(&path, profile, seen_paths, items);
            }
        }
    }
}

fn maybe_add_catalog_path(
    path: &Path,
    profile: &CatalogProfile,
    seen_paths: &mut HashSet<String>,
    items: &mut Vec<CatalogItem>,
) {
    if !catalog_path_matches(path, profile) {
        return;
    }

    let path_key = normalize_path_key(path);
    if !seen_paths.insert(path_key.clone()) {
        return;
    }

    let label = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| path.display().to_string());
    let display_path = path.to_string_lossy().to_string();
    let category = if path.is_dir() {
        ItemCategory::Folder
    } else {
        ItemCategory::File
    };
    let mut keywords = Vec::new();
    if !profile.label.trim().is_empty() {
        keywords.push(profile.label.clone());
    }
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        keywords.push(extension.to_string());
    }

    let profile_id = non_empty(&profile.id).unwrap_or("default");
    items.push(
        CatalogItem::new(
            format!("file_catalog:{profile_id}:{path_key}"),
            label,
            category,
            "file_catalog",
        )
        .subtitle(display_path.clone())
        .keywords(keywords)
        .action(Action::open_file(display_path)),
    );
}

fn catalog_path_matches(path: &Path, profile: &CatalogProfile) -> bool {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let path_text = normalize_match_text(&path.to_string_lossy());

    if !profile.include_patterns.is_empty()
        && !profile
            .include_patterns
            .iter()
            .any(|pattern| catalog_pattern_matches(pattern, file_name, &path_text))
    {
        return false;
    }

    !profile
        .exclude_patterns
        .iter()
        .any(|pattern| catalog_pattern_matches(pattern, file_name, &path_text))
}

fn catalog_pattern_matches(pattern: &str, file_name: &str, path_text: &str) -> bool {
    let pattern = normalize_match_text(pattern);
    let target = if pattern.contains('/') {
        path_text.to_string()
    } else {
        normalize_match_text(file_name)
    };

    wildcard_matches(&pattern, &target)
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = normalize_case(pattern);
    let value = normalize_case(value);
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_match_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            star_match_index = value_index;
            pattern_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_match_index += 1;
            value_index = star_match_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

fn normalize_match_text(value: &str) -> String {
    value.replace('\\', "/")
}

fn normalize_case(value: &str) -> String {
    #[cfg(windows)]
    return value.to_ascii_lowercase();

    #[cfg(not(windows))]
    return value.to_string();
}

fn expand_path(value: &str) -> PathBuf {
    let mut expanded = expand_percent_env(value);
    expanded = expand_dollar_env(&expanded);

    if let Some(rest) = expanded.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    {
        return home.join(rest);
    }

    PathBuf::from(expanded)
}

fn expand_percent_env(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push('%');
            output.push_str(after_start);
            return output;
        };

        let name = &after_start[..end];
        if let Some(replacement) = std::env::var_os(name) {
            output.push_str(&replacement.to_string_lossy());
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        rest = &after_start[end + 1..];
    }

    output.push_str(rest);
    output
}

fn expand_dollar_env(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }

        if chars.peek() == Some(&'{') {
            chars.next();
            let mut name = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                name.push(next);
            }
            push_env_or_literal(&mut output, &name, true);
            continue;
        }

        let mut name = String::new();
        while let Some(next) = chars.peek().copied() {
            if next == '_' || next.is_ascii_alphanumeric() {
                name.push(next);
                chars.next();
            } else {
                break;
            }
        }

        if name.is_empty() {
            output.push('$');
        } else {
            push_env_or_literal(&mut output, &name, false);
        }
    }

    output
}

fn push_env_or_literal(output: &mut String, name: &str, braced: bool) {
    if let Some(replacement) = std::env::var_os(name) {
        output.push_str(&replacement.to_string_lossy());
    } else if braced {
        output.push_str("${");
        output.push_str(name);
        output.push('}');
    } else {
        output.push('$');
        output.push_str(name);
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn discover_path_executables_with_seen(
    seen_by_name: &mut HashSet<String>,
    seen_by_path: &mut HashSet<String>,
) -> Vec<CatalogItem> {
    let path_dirs = split_path_entries();
    let extensions = executable_extensions();
    let mut items = Vec::new();

    for dir in path_dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_executable_file(&path, &extensions) {
                    continue;
                }

                let file_name = match path.file_name().and_then(|value| value.to_str()) {
                    Some(file_name) => file_name,
                    None => continue,
                };

                let normalized_name = normalize_executable_name(file_name);
                if !seen_by_name.insert(normalized_name.clone()) {
                    continue;
                }

                let path_key = normalize_path_key(&path);
                if !seen_by_path.insert(path_key.clone()) {
                    continue;
                }

                let command = path.to_string_lossy().to_string();
                items.push(
                    CatalogItem::new(
                        format!("path_executable:{path_key}"),
                        file_name,
                        ItemCategory::Command,
                        "path",
                    )
                    .subtitle(command.clone())
                    .keywords([normalized_name])
                    .action(Action::launch(command)),
                );
            }
        }
    }

    items
}

fn split_path_entries() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|entry| entry.is_dir())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn executable_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        let configured = std::env::var_os("PATHEXT")
            .as_ref()
            .map(|value| value.to_string_lossy())
            .as_deref()
            .map(parse_executable_extensions)
            .unwrap_or_default();

        if !configured.is_empty() {
            return configured;
        }

        WINDOWS_DEFAULT_PATHEXT
            .iter()
            .map(|item| item.to_string())
            .collect()
    }

    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(any(windows, test))]
fn parse_executable_extensions(value: &str) -> Vec<String> {
    let mut extensions = Vec::new();

    for entry in value.split(';') {
        let extension = entry.trim();
        if extension.is_empty() {
            continue;
        }

        let mut normalized = extension.to_ascii_uppercase();
        if !normalized.starts_with('.') {
            normalized.insert(0, '.');
        }
        if !extensions.contains(&normalized) {
            extensions.push(normalized);
        }
    }

    extensions
}

fn normalize_executable_name(value: &str) -> String {
    #[cfg(windows)]
    return value.to_ascii_lowercase();

    #[cfg(not(windows))]
    return value.to_string();
}

fn normalize_path_key(path: &Path) -> String {
    let key = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    #[cfg(windows)]
    return key.to_ascii_lowercase();

    #[cfg(not(windows))]
    return key;
}

#[cfg(windows)]
fn is_executable_file(path: &Path, extensions: &[String]) -> bool {
    if !path.is_file() {
        return false;
    }

    let extension = match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => extension,
        None => return false,
    };
    let extension = format!(".{}", extension.to_ascii_uppercase());
    extensions.iter().any(|entry| entry == &extension)
}

#[cfg(windows)]
fn discover_start_menu_shortcuts(
    seen_by_name: &mut HashSet<String>,
    seen_by_path: &mut HashSet<String>,
) -> Vec<CatalogItem> {
    let mut items = Vec::new();
    let mut roots = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        roots.push(PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut queue = VecDeque::from([root]);
        while let Some(dir) = queue.pop_front() {
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };

                if file_type.is_dir() {
                    queue.push_back(path);
                    continue;
                }

                if !file_type.is_file() || !is_shortcut_file(&path) {
                    continue;
                }

                let label = match path.file_stem().and_then(|value| value.to_str()) {
                    Some(file_name) => file_name,
                    None => continue,
                };
                let normalized_name = normalize_executable_name(label);

                if !seen_by_name.insert(normalized_name.clone()) {
                    continue;
                }

                let path_key = normalize_path_key(&path);
                if !seen_by_path.insert(path_key.clone()) {
                    continue;
                }

                items.push(
                    CatalogItem::new(
                        format!("start_menu_shortcut:{path_key}"),
                        label,
                        ItemCategory::App,
                        "start_menu",
                    )
                    .subtitle(path.to_string_lossy().to_string())
                    .keywords([normalized_name])
                    .action(shortcut_launch_action(&path)),
                );
            }
        }
    }

    items
}

#[cfg(not(windows))]
fn discover_start_menu_shortcuts(
    _seen_by_name: &mut HashSet<String>,
    _seen_by_path: &mut HashSet<String>,
) -> Vec<CatalogItem> {
    Vec::new()
}

#[cfg(windows)]
fn is_shortcut_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

#[cfg(windows)]
fn shortcut_launch_action(path: &Path) -> Action {
    let shortcut = path.to_string_lossy().to_string();
    Action {
        id: "default".to_string(),
        label: "Open".to_string(),
        kind: ActionKind::ShellCommand,
        command: Some("cmd".to_string()),
        args: vec!["/C".into(), "start".into(), "".into(), shortcut],
        requires_confirmation: false,
        run_as_admin: false,
    }
}

#[cfg(not(windows))]
fn is_executable_file(path: &Path, _extensions: &[String]) -> bool {
    #[cfg(unix)]
    {
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn portable_profile_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let portable = exe.parent()?.join("portable");
    portable.is_dir().then_some(portable)
}

pub fn execute_action(action: &Action) -> Result<(), PlatformError> {
    match action.kind {
        ActionKind::Launch | ActionKind::ShellCommand => spawn_command(action),
        ActionKind::OpenFile => {
            let path = action
                .command
                .as_deref()
                .ok_or(PlatformError::MissingCommand)?;
            open_path(path)
        }
        ActionKind::OpenUrl => {
            let url = action
                .command
                .as_deref()
                .ok_or(PlatformError::MissingCommand)?;
            open_url(url)
        }
        ActionKind::AiPrompt | ActionKind::ToolCall => Err(PlatformError::UnsupportedAction {
            kind: format!("{:?}", action.kind),
        }),
    }
}

fn spawn_command(action: &Action) -> Result<(), PlatformError> {
    let command = action
        .command
        .as_deref()
        .ok_or(PlatformError::MissingCommand)?;

    let mut process = Command::new(command);
    process.args(&action.args);

    spawn_without_console(process, command)
}

fn open_url(url: &str) -> Result<(), PlatformError> {
    open_with_system_handler(url)
}

fn open_path(path: &str) -> Result<(), PlatformError> {
    open_with_system_handler(path)
}

fn open_with_system_handler(target: &str) -> Result<(), PlatformError> {
    let command = if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", target]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(target);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(target);
        command
    };

    spawn_without_console(command, target)
}

fn spawn_without_console(mut command: Command, command_label: &str) -> Result<(), PlatformError> {
    suppress_console_window(&mut command);

    command
        .spawn()
        .map(|_| ())
        .map_err(|source| PlatformError::SpawnFailed {
            command: command_label.to_string(),
            source,
        })
}

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("action has no command")]
    MissingCommand,
    #[error("unsupported action kind: {kind}")]
    UnsupportedAction { kind: String },
    #[error("failed to spawn `{command}`")]
    SpawnFailed {
        command: String,
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        catalog_path_matches, discover_file_catalog_items, expand_dollar_env, expand_percent_env,
        normalize_executable_name, normalize_path_key, parse_executable_extensions,
        wildcard_matches,
    };
    #[cfg(windows)]
    use super::{is_shortcut_file, shortcut_launch_action};
    use veyra_core::config::CatalogProfile;
    use veyra_core::{ActionKind, ItemCategory};

    #[test]
    fn parses_pathext_with_case_and_spaces() {
        let extensions = parse_executable_extensions(".exE; .cmd; .Exe;; .bat");
        assert_eq!(extensions, vec![".EXE", ".CMD", ".BAT"]);
    }

    #[test]
    fn parses_empty_pathext_to_empty_list() {
        assert!(parse_executable_extensions("").is_empty());
    }

    #[test]
    fn normalize_executable_name_matches_platform_case_rules() {
        #[cfg(windows)]
        assert_eq!(normalize_executable_name("FooBar.CMD"), "foobar.cmd");
        #[cfg(not(windows))]
        assert_eq!(normalize_executable_name("FooBar.CMD"), "FooBar.CMD");
    }

    #[test]
    fn path_keys_are_stable_for_duplicate_paths() {
        let path = PathBuf::from("a").join("b").join("binary");
        assert_eq!(normalize_path_key(&path), normalize_path_key(&path));
    }

    #[test]
    fn dedupe_logic_prefers_first_occurrence() {
        let candidates = [
            PathBuf::from("first/cmd.exe"),
            PathBuf::from("second/cmd.exe"),
            PathBuf::from("first/cmd.exe"),
        ];

        let mut seen_names = HashSet::new();
        let mut seen_paths = HashSet::new();
        let mut selected = Vec::new();

        for candidate in &candidates {
            let file_name = candidate
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap();
            let name_key = normalize_executable_name(file_name);
            let path_key = normalize_path_key(candidate);

            if seen_names.insert(name_key) && seen_paths.insert(path_key) {
                selected.push(candidate);
            }
        }

        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn wildcard_matching_supports_simple_globs() {
        assert!(wildcard_matches("*.md", "readme.md"));
        assert!(wildcard_matches("config.?oml", "config.toml"));
        assert!(wildcard_matches(
            "*/node_modules/*",
            "repo/node_modules/pkg"
        ));
        assert!(!wildcard_matches("*.md", "readme.txt"));
    }

    #[test]
    fn catalog_path_matching_honors_include_and_exclude_patterns() {
        let profile = CatalogProfile {
            include_patterns: vec!["*.md".to_string()],
            exclude_patterns: vec!["secret*".to_string()],
            ..Default::default()
        };

        assert!(catalog_path_matches(Path::new("README.md"), &profile));
        assert!(!catalog_path_matches(Path::new("notes.txt"), &profile));
        assert!(!catalog_path_matches(Path::new("secret-plan.md"), &profile));
    }

    #[test]
    fn env_expansion_preserves_unknown_variables() {
        assert_eq!(
            expand_percent_env("%VEYRA_TEST_UNKNOWN%/dir"),
            "%VEYRA_TEST_UNKNOWN%/dir"
        );
        assert_eq!(
            expand_dollar_env("${VEYRA_TEST_UNKNOWN}/dir"),
            "${VEYRA_TEST_UNKNOWN}/dir"
        );
    }

    #[test]
    fn file_catalog_discovery_indexes_filtered_files_with_depth() {
        let root = temp_catalog_dir();
        let subdir = root.join("sub");
        let deep = subdir.join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(root.join("readme.md"), "root").unwrap();
        fs::write(root.join("skip.tmp"), "tmp").unwrap();
        fs::write(subdir.join("note.md"), "sub").unwrap();
        fs::write(deep.join("too_deep.md"), "deep").unwrap();

        let profile = CatalogProfile {
            id: "docs".to_string(),
            label: "Docs".to_string(),
            paths: vec![
                root.display().to_string(),
                root.join("missing").display().to_string(),
            ],
            include_patterns: vec!["*.md".to_string()],
            exclude_patterns: vec!["too_*".to_string()],
            recursive: true,
            max_depth: Some(1),
            enabled: true,
            ..Default::default()
        };
        let disabled = CatalogProfile {
            enabled: false,
            paths: vec![root.display().to_string()],
            ..Default::default()
        };

        let discovery = discover_file_catalog_items(&[profile, disabled]);
        let mut labels = discovery
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        labels.sort_unstable();

        assert_eq!(labels, vec!["note.md", "readme.md"]);
        assert_eq!(discovery.enabled_profiles, 1);
        assert_eq!(discovery.skipped_profiles, 1);
        assert_eq!(discovery.skipped_paths, 1);
        assert!(
            discovery
                .items
                .iter()
                .all(|item| item.category == ItemCategory::File)
        );
        assert!(discovery.items.iter().all(|item| {
            item.actions
                .first()
                .is_some_and(|action| action.kind == ActionKind::OpenFile)
        }));

        fs::remove_dir_all(root).ok();
    }

    #[cfg(windows)]
    #[test]
    fn detects_windows_shortcut_file_extension() {
        assert!(is_shortcut_file(Path::new("Notepad.lnk")));
        assert!(is_shortcut_file(Path::new("notepad.LNK")));
        assert!(!is_shortcut_file(Path::new("notepad.exe")));
    }

    #[cfg(windows)]
    #[test]
    fn creates_shell_command_action_for_shortcut() {
        let action = shortcut_launch_action(Path::new(r"C:\ProgramData\Microsoft\Windows\foo.lnk"));

        assert_eq!(action.command.as_deref(), Some("cmd"));
        assert_eq!(action.kind, ActionKind::ShellCommand);
        assert_eq!(
            action.args,
            vec![
                "/C",
                "start",
                "",
                r"C:\ProgramData\Microsoft\Windows\foo.lnk"
            ]
        );
    }

    fn temp_catalog_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("veyra-platform-catalog-test-{nanos}"));
        path
    }
}

use std::path::PathBuf;

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

fn portable_profile_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let portable = exe.parent()?.join("portable");
    portable.is_dir().then_some(portable)
}

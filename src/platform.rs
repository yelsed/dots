use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Linux => write!(f, "linux"),
            Platform::Macos => write!(f, "macos"),
            Platform::Windows => write!(f, "windows"),
        }
    }
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux // fallback
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "linux" => Some(Platform::Linux),
            "macos" | "darwin" => Some(Platform::Macos),
            "windows" | "win" => Some(Platform::Windows),
            _ => None,
        }
    }
}

/// Check if an entry's platform list includes the current platform
pub fn is_relevant(platforms: &[Platform]) -> bool {
    platforms.contains(&Platform::current())
}

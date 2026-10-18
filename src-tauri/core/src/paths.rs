use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const APP_DIR: &str = "octopus-chat";

/// Canonicalises a folder and returns it in the form the rest of the app stores
/// and compares against.
///
/// Two things happen here that are easy to get wrong separately:
///
/// 1. The path is resolved, so `C:\a\..\b` and symlinked spellings collapse to
///    one identity. Containment checks later compare against this string, so the
///    resolution has to happen once, up front.
/// 2. The Windows extended-length prefix is stripped. `canonicalize` returns
///    `\\?\C:\work\app`, which is correct for syscalls but wrong to store: the
///    sandbox resolves tool arguments to ordinary `C:\...` paths, so a root held
///    as `\\?\C:\work\app` fails its own containment check and denies every read
///    and write. It is also unreadable in the project picker.
///
/// Returns the input unchanged if it does not exist, so callers that only want
/// normalisation can use this on paths they have not verified.
pub fn canonical_folder(path: &Path) -> Result<String> {
    if !path.exists() {
        return Err(Error::invalid(format!("No such folder: {}", path.display())));
    }
    if !path.is_dir() {
        return Err(Error::invalid(format!("Not a folder: {}", path.display())));
    }
    let resolved = path
        .canonicalize()
        .map_err(|e| Error::io(format!("{}: {e}", path.display())))?;
    Ok(strip_extended_prefix(&resolved.to_string_lossy()))
}

/// Removes the `\\?\` (or `\\?\UNC\`) prefix and normalises separators.
pub fn strip_extended_prefix(path: &str) -> String {
    let text = path
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(str::to_owned))
        .unwrap_or_else(|| path.to_owned());

    if cfg!(windows) { text.replace('/', "\\") } else { text }
}

fn base() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|d| d.join(APP_DIR))
        .ok_or_else(|| Error::msg("could not resolve a data directory for this user"))
}

/// Per-user mutable state: database, logs, caches.
pub fn data_dir() -> Result<PathBuf> {
    let d = base()?;
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("octopus.db"))
}

pub fn logs_dir() -> Result<PathBuf> {
    let d = data_dir()?.join("logs");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// Checkpoints used by fork & rewind to restore the workspace.
pub fn checkpoints_dir() -> Result<PathBuf> {
    let d = data_dir()?.join("checkpoints");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// Artifacts written by the agent (html, images, csv, ...).
pub fn artifacts_dir() -> Result<PathBuf> {
    let d = data_dir()?.join("artifacts");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// User-authored, hand-editable files: custom providers, themes, skills.
pub fn config_dir() -> Result<PathBuf> {
    let d = dirs::config_dir()
        .map(|c| c.join(APP_DIR))
        .ok_or_else(|| Error::msg("could not resolve a config directory for this user"))?;
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn themes_dir() -> Result<PathBuf> {
    let d = config_dir()?.join("themes");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn skills_dir() -> Result<PathBuf> {
    let d = config_dir()?.join("skills");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn agents_dir() -> Result<PathBuf> {
    let d = config_dir()?.join("agents");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// Normalizes a possibly relative user-supplied path against a base directory
/// and refuses to escape it, so agent-supplied paths stay inside the workspace.
pub fn resolve_within(base: &Path, candidate: &str) -> Result<PathBuf> {
    let raw = Path::new(candidate);
    let joined =
        if raw.is_absolute() { raw.to_path_buf() } else { base.join(raw) };
    let normalized = normalize(&joined);
    let base_norm = normalize(base);
    if !normalized.starts_with(&base_norm) {
        return Err(Error::Denied(format!("path escapes the workspace: {candidate}")));
    }
    Ok(normalized)
}

/// Lexical path normalization: resolves `.` and `..` without touching the disk,
/// so it works for paths that do not exist yet.
pub fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() { PathBuf::from(".") } else { out }
}

/// Shortens a path for display: `C:/work/octopus-chat/src/lib.rs` becomes
/// `~/octopus-chat/src/lib.rs` when it sits under the user's home directory.
pub fn display_path(p: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = p.strip_prefix(&home)
    {
        return format!("~/{}", rel.to_string_lossy().replace('\\', "/"));
    }
    p.to_string_lossy().into_owned()
}

/// The folders the app reads and writes, surfaced in Settings so the user can
/// see exactly what lives on their machine.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub data: String,
    pub database: String,
    pub config: String,
    pub themes: String,
    pub artifacts: String,
    pub logs: String,
    pub checkpoints: String,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        Ok(Self {
            data: data_dir()?.to_string_lossy().into_owned(),
            database: db_path()?.to_string_lossy().into_owned(),
            config: config_dir()?.to_string_lossy().into_owned(),
            themes: themes_dir()?.to_string_lossy().into_owned(),
            artifacts: artifacts_dir()?.to_string_lossy().into_owned(),
            logs: logs_dir()?.to_string_lossy().into_owned(),
            checkpoints: checkpoints_dir()?.to_string_lossy().into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_length_prefix_is_removed() {
        assert_eq!(strip_extended_prefix(r"\\?\C:\work\app"), r"C:\work\app");
        assert_eq!(strip_extended_prefix(r"\\?\D:\a\b"), r"D:\a\b");
    }

    #[test]
    fn unc_prefix_is_rewritten_to_a_plain_unc_path() {
        assert_eq!(strip_extended_prefix(r"\\?\UNC\server\share\p"), r"\\server\share\p");
    }

#[test]
    fn ordinary_paths_pass_through_unchanged() {
        assert_eq!(strip_extended_prefix(r"C:\work\app"), r"C:\work\app");
    }

    #[test]
    fn separators_are_normalised_per_platform() {
        // Storing one separator style is what makes the later string
        // containment check reliable, so a path written with forward slashes
        // has to be rewritten on Windows and left alone elsewhere.
        let got = strip_extended_prefix("C:/work/app");
        if cfg!(windows) {
            assert_eq!(got, r"C:\work\app");
        } else {
            assert_eq!(got, "C:/work/app");
        }
    }

    #[test]
    fn canonical_folder_never_returns_an_extended_length_path() {
        // Whatever the platform hands back, the stored root must be comparable
        // with the plain paths the sandbox produces.
        let here = Path::new(".").canonicalize().expect("cwd should resolve");
        let got = canonical_folder(&here).expect("cwd is a folder");
        assert!(!got.starts_with(r"\\?\"), "got {got}");
    }

    #[test]
    fn canonical_folder_rejects_a_file() {
        let f = std::env::current_exe().expect("test binary path");
        assert!(canonical_folder(&f).is_err(), "a file must not pass as a folder");
    }

    #[test]
    fn canonical_folder_rejects_a_missing_path() {
        let missing = Path::new("definitely-not-here-9f2a");
        assert!(canonical_folder(&missing).is_err());
    }
}

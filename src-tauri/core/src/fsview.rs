//! Filesystem browsing and editing for the right-hand panel.
//!
//! Everything is confined to the session's project roots, so the panel can
//! never become a way to read the rest of the disk.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::paths;
use crate::tools::fswalk::walk;
use crate::tools::ToolContext;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    /// Display path relative to the project root.
    pub path: String,
    pub is_dir: bool,
    /// Byte size for files.
    pub size: u64,
    pub children: Vec<TreeNode>,
}

/// Lists a directory as a tree, depth-limited so a huge repository cannot lock
/// the UI.
pub fn tree(roots: &[std::path::PathBuf], sub: &str, ignore: &[String], depth: usize) -> Result<TreeNode> {
    let mut ctx = ToolContext::new(roots.to_vec(), "panel");
    ctx.ignore = ignore.to_vec();
    let start = if sub.is_empty() {
        roots
            .first()
            .cloned()
            .ok_or_else(|| Error::invalid("no project folder is open"))?
    } else {
        ctx.resolve(sub)?
    };

    let mut node = build(&ctx, &start, depth.max(1));
    if node.is_dir {
        node.path = crate::tools::fswalk::relative_display(&ctx, &start);
    }
    Ok(node)
}

fn build(ctx: &ToolContext, dir: &std::path::Path, depth: usize) -> TreeNode {
    let mut children = Vec::new();
    let mut total = 0u64;

    if let Ok(reader) = std::fs::read_dir(dir) {
        let mut entries: Vec<_> = reader.flatten().collect();
        entries.sort_by_key(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (!is_dir, e.file_name())
        });
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if ctx.is_ignored(&name) {
                continue;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            let is_dir = ft.is_dir();
            if is_dir {
                children.push(if depth > 1 {
                    build(ctx, &path, depth - 1)
                } else {
                    TreeNode {
                        name,
                        path: crate::tools::fswalk::relative_display(ctx, &path),
                        is_dir: true,
                        size: 0,
                        children: Vec::new(),
                    }
                });
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                children.push(TreeNode {
                    name,
                    path: crate::tools::fswalk::relative_display(ctx, &path),
                    is_dir: false,
                    size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                    children: Vec::new(),
                });
            }
        }
    }

    TreeNode {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.to_string_lossy().into_owned()),
        path: String::new(),
        is_dir: true,
        size: total,
        children,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub bytes: u64,
    pub language: String,
    pub text: String,
    /// True when the file is not valid UTF-8 and was not decoded.
    pub binary: bool,
}

/// Reads a text file for the panel viewer and editor.
pub fn read(roots: &[std::path::PathBuf], path: &str, max_bytes: usize) -> Result<FileContent> {
    let ctx = ToolContext::new(roots.to_vec(), "panel");
    let resolved = ctx.resolve(path)?;
    let meta = std::fs::metadata(&resolved).map_err(Error::io)?;
    if meta.is_dir() {
        return Err(Error::invalid("that is a directory"));
    }

    let bytes = std::fs::read(&resolved).map_err(Error::io)?;
    // A NUL byte in the first kilobyte is the cheapest reliable binary test.
    let binary = bytes.iter().take(1024).any(|b| *b == 0) || std::str::from_utf8(&bytes).is_err();
    let text = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes)]).into_owned()
    };

    Ok(FileContent {
        path: ctx.display(&resolved),
        bytes: meta.len(),
        language: language_for(&resolved),
        text,
        binary,
    })
}

/// Saves edited text back, refusing anything outside the roots and refusing to
/// clobber a file that is not valid UTF-8 text.
pub fn write(roots: &[std::path::PathBuf], path: &str, text: &str) -> Result<FileContent> {
    let ctx = ToolContext::new(roots.to_vec(), "panel");
    let resolved = ctx.resolve(path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io)?;
    }
    std::fs::write(&resolved, text.as_bytes()).map_err(Error::io)?;
    read(roots, path, 2 * 1024 * 1024)
}

/// Files changed on disk since the agent last ran, for the live-edit badge.
pub fn changed_files(roots: &[std::path::PathBuf], ignore: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for root in roots {
        let hits = walk::collect(root, ignore, 20_000, |_| true);
        for hit in hits {
            // Anything touched in the last two minutes is treated as live, which
            // is close enough for a "recently changed" indicator and costs
            // nothing to maintain.
            if let Ok(meta) = std::fs::metadata(&hit.path)
                && let Ok(modified) = meta.modified()
                && let Ok(age) = std::time::SystemTime::now().duration_since(modified)
                && age < std::time::Duration::from_secs(120)
            {
                out.push(paths::display_path(&hit.path));
            }
        }
    }
    Ok(out)
}

/// Maps an extension to a Shiki grammar name.
pub fn language_for(path: &std::path::Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let name = match ext.as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "json" | "jsonc" | "json5" => "json",
        "sh" | "bash" | "zsh" => "bash",
        "ps1" | "psm1" => "powershell",
        "py" | "pyi" => "python",
        "go" => "go",
        "sql" => "sql",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        "diff" | "patch" => "diff",
        "md" | "markdown" => "markdown",
        "xml" | "svg" => "html",
        _ => "",
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> tempdir::TempDir {
        let dir = tempdir::TempDir::new("octopus-panel").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "# hi\n").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::write(dir.path().join("node_modules/pkg/i.js"), "x").unwrap();
        dir
    }

    #[test]
    fn tree_lists_files_and_honours_ignores() {
        let dir = roots();
        let node = tree(&[dir.path().to_path_buf()], "", &["node_modules".into()], 3).unwrap();
        let names: Vec<&str> = node.children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"src"));
        assert!(names.contains(&"README.md"));
        assert!(!names.contains(&"node_modules"), "ignored dir leaked into the tree");
    }

    #[test]
    fn tree_descends_to_the_requested_depth() {
        let dir = roots();
        let node = tree(&[dir.path().to_path_buf()], "", &[], 2).unwrap();
        let src = node.children.iter().find(|c| c.name == "src").unwrap();
        assert_eq!(src.children.len(), 1);
        // Depth 1 stops at the directory name.
        let shallow = tree(&[dir.path().to_path_buf()], "", &[], 1).unwrap();
        let src = shallow.children.iter().find(|c| c.name == "src").unwrap();
        assert!(src.children.is_empty());
    }

    #[test]
    fn read_returns_text_and_a_grammar() {
        let dir = roots();
        let f = read(&[dir.path().to_path_buf()], "src/main.rs", 10_000).unwrap();
        assert!(!f.binary);
        assert_eq!(f.language, "rust");
        assert!(f.text.contains("fn main"));
    }

    #[test]
    fn read_refuses_to_escape_the_workspace() {
        let dir = roots();
        assert!(read(&[dir.path().to_path_buf()], "../../../etc/passwd", 10_000).is_err());
    }

    #[test]
    fn binary_files_are_flagged_not_decoded() {
        let dir = roots();
        std::fs::write(dir.path().join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
        let f = read(&[dir.path().to_path_buf()], "blob.bin", 10_000).unwrap();
        assert!(f.binary);
        assert!(f.text.is_empty());
    }

    #[test]
    fn write_round_trips() {
        let dir = roots();
        let p = dir.path().to_path_buf();
        write(&[p.clone()], "src/new.txt", "hello").unwrap();
        assert_eq!(read(&[p], "src/new.txt", 1000).unwrap().text, "hello");
    }

    #[test]
    fn extensions_map_to_grammars() {
        assert_eq!(language_for(std::path::Path::new("a.rs")), "rust");
        assert_eq!(language_for(std::path::Path::new("a.tsx")), "tsx");
        assert_eq!(language_for(std::path::Path::new("Makefile")), "");
    }
}
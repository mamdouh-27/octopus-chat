use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{arg_opt_str, arg_str, arg_u32, Display, Tool, ToolContext, ToolFuture, ToolOutcome};
use crate::error::{Error, Result};
use crate::util;

/// Reads a file, numbered like `cat -n` so the model can refer to exact lines.
pub struct ReadFile;

fn schema_props() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "File path, absolute or relative to the workspace root."
            },
            "offset": {
                "type": "integer",
                "description": "1-based first line to return. Defaults to the start of the file."
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of lines to return. Defaults to 2000."
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read a text file from the workspace. Returns the content with 1-based line \
         numbers. Use offset and limit for large files."
    }

    fn parameters(&self) -> Value {
        schema_props()
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => format!("Read {p}"),
            None => "Read a file".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let raw = arg_str(&args, "path")?;
            let path = ctx.resolve(raw)?;
            let offset = arg_u32(&args, "offset", 1).max(1) as usize;
            let limit = arg_u32(&args, "limit", 2000).max(1) as usize;

            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| Error::io(format!("{}: {e}", ctx.display(&path))))?;
            if meta.is_dir() {
                return Err(Error::invalid(format!(
                    "{} is a directory; use the list tool",
                    ctx.display(&path)
                )));
            }

            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| Error::io(format!("{}: {e}", ctx.display(&path))))?;
            let text = String::from_utf8_lossy(&bytes);
            let body = number_lines(&text, offset, limit);
            let shown = body.lines().count();
            let total = text.lines().count();

            let mut content = body;
            if shown < total {
                content.push_str(&format!(
                    "\n\n[showing lines {}-{} of {total}; pass offset to continue]",
                    offset,
                    offset + shown - 1
                ));
            }

            Ok(ToolOutcome::text(ctx.clip(content)).with_display(Display::File {
                path: ctx.display(&path),
                bytes: meta.len(),
                preview: util::truncate(&text, 4_000),
            }))
        })
    }
}

/// `cat -n` semantics with 1-based numbering. The gutter is sized from the
/// widest number actually shown, so a short read of a huge file stays narrow.
fn number_lines(text: &str, offset: usize, limit: usize) -> String {
    let selected: Vec<&str> = text.lines().skip(offset - 1).take(limit).collect();
    if selected.is_empty() {
        return String::new();
    }
    let width = (offset + selected.len() - 1).to_string().len();
    let mut out = String::new();
    for (i, line) in selected.iter().enumerate() {
        out.push_str(&util::pad(&(offset + i).to_string(), width));
        out.push('\t');
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Lists a directory. Directories are annotated with a trailing slash.
pub struct ListDir;

impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List the entries of a directory in the workspace."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list. Defaults to the workspace root."
                }
            },
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => format!("List {p}"),
            None => "List the workspace".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let raw = arg_opt_str(&args, "path").unwrap_or(".");
            let dir = ctx.resolve(raw)?;

            // `tokio::fs::ReadDir` is a stream, so entries are pulled one at a
            // time instead of being materialised eagerly.
            let mut reader = tokio::fs::read_dir(&dir)
                .await
                .map_err(|e| Error::io(format!("{}: {e}", ctx.display(&dir))))?;
            let mut entries: Vec<tokio::fs::DirEntry> = Vec::new();
            while let Some(entry) = reader
                .next_entry()
                .await
                .map_err(|e| Error::io(format!("{}: {e}", ctx.display(&dir))))?
            {
                entries.push(entry);
            }

            // Classify first so directories sort above files, then render.
            let mut rows: Vec<(bool, String, String)> = Vec::with_capacity(entries.len());
            for e in &entries {
                let name = e.file_name().to_string_lossy().into_owned();
                if ctx.is_ignored(&name) {
                    continue;
                }
                match e.file_type().await {
                    Ok(t) if t.is_dir() => rows.push((false, format!("{name}/"), String::new())),
                    Ok(t) if t.is_symlink() => rows.push((false, format!("{name}@"), String::new())),
                    Ok(_) => {
                        let len = e.metadata().await.map(|m| m.len()).unwrap_or(0);
                        rows.push((true, name, human_bytes(len)));
                    }
                    Err(_) => rows.push((true, name, String::new())),
                }
            }
            rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

            let mut out = String::new();
            for (_, name, size) in &rows {
                if size.is_empty() {
                    out.push_str(name);
                } else {
                    out.push_str(&format!("{name}\t{size}"));
                }
                out.push('\n');
            }
            if out.is_empty() {
                out.push_str("(empty)");
            } else {
                out.push_str(&format!("\n{} entries", rows.len()));
            }
            Ok(ToolOutcome::text(ctx.clip(out)))
        })
    }
}

/// Finds files by glob pattern, honouring the ignore list.
pub struct Glob;

impl Tool for Glob {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> &'static str {
        "Find files matching a glob pattern such as `src/**/*.rs`. Results are sorted \
         by modification time, newest first."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern." },
                "path": { "type": "string", "description": "Directory to search under." }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => format!("Find files matching {p}"),
            None => "Find files".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let pattern = arg_str(&args, "pattern")?;
            let base = ctx.resolve(arg_opt_str(&args, "path").unwrap_or("."))?;
            let mut hits = walk::collect(&base, &ctx.ignore, 20_000, |p| {
                walk::glob_match_rel(pattern, &base, p)
            });

            hits.sort_by_key(|m| std::cmp::Reverse(m.modified));
            let total = hits.len();
            let shown: Vec<String> = hits.iter().take(500).map(|m| ctx.display(&m.path)).collect();

            let mut content = shown.join("\n");
            if total > shown.len() {
                content.push_str(&format!("\n\n[{} of {total} matches shown]", shown.len()));
            }
            if total == 0 {
                content = format!("No files matched {pattern}");
            }
            Ok(ToolOutcome::text(ctx.clip(content))
                .with_display(Display::Matches { count: total }))
        })
    }
}

/// Content search across the workspace.
pub struct Grep {
    pub ignore: Vec<String>,
}

impl Grep {
    fn compiled(&self, pattern: &str) -> Result<regex::Regex> {
        regex::Regex::new(pattern)
            .map_err(|e| Error::invalid(format!("invalid regular expression: {e}")))
    }
}

impl Tool for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search file contents with a regular expression. Returns matching lines with \
         their file path and line number."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Rust regular expression." },
                "path": { "type": "string", "description": "File or directory to search." },
                "glob": { "type": "string", "description": "Only search files matching this glob." },
                "case_sensitive": { "type": "boolean", "description": "Defaults to false." },
                "max_results": { "type": "integer", "description": "Defaults to 200." }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn summary(&self, args: &Value) -> String {
        match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => format!("Search for /{p}/"),
            None => "Search files".into(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let pattern = arg_str(&args, "pattern")?;
            let re = self.compiled(pattern)?;
            let target = ctx.resolve(arg_opt_str(&args, "path").unwrap_or("."))?;
            let filter = arg_opt_str(&args, "glob").map(str::to_string);
            let limit = arg_u32(&args, "max_results", 200) as usize;

            let re = if super::arg_bool(&args, "case_sensitive", false) {
                re
            } else {
                regex::RegexBuilder::new(&re.as_str())
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| Error::invalid(e.to_string()))?
            };

            let files = if target.is_file() {
                vec![target.clone()]
            } else {
                walk::collect(&target, &self.ignore, 50_000, |p| {
                    filter.as_deref().is_none_or(|g| walk::glob_match_rel(g, &target, p))
                })
                .into_iter()
                .map(|m| m.path)
                .collect()
            };

            let mut lines: Vec<String> = Vec::new();
            let mut total = 0usize;
            for f in files {
                let Ok(bytes) = tokio::fs::read(&f).await else { continue };
                // Skip binaries rather than dumping mojibake into the context.
                if bytes.contains(&0) {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                for (i, line) in text.lines().enumerate() {
                    if !re.is_match(line) {
                        continue;
                    }
                    total += 1;
                    if lines.len() < limit {
                        let shown = util::truncate(line.trim(), 300);
                        lines.push(format!("{}:{}: {}", ctx.display(&f), i + 1, shown));
                    }
                }
            }

            let mut content = lines.join("\n");
            if total > lines.len() {
                content.push_str(&format!("\n\n[{} of {total} matches shown]", lines.len()));
            }
            if total == 0 {
                content = format!("No matches for /{pattern}/");
            }
            Ok(ToolOutcome::text(ctx.clip(content))
                .with_display(Display::Matches { count: total }))
        })
    }
}

/// Shared, ignore-aware directory walk.
pub mod walk {
    use super::*;
    use std::time::SystemTime;

    pub struct Hit {
        pub path: PathBuf,
        pub modified: SystemTime,
    }

    /// Walks `base`, skipping ignored directory names and binary files, and
    /// keeps entries where `keep` returns true. `cap` bounds the walk so a
    /// runaway tree cannot hang the agent.
    pub fn collect(
        base: &Path,
        ignore: &[String],
        cap: usize,
        keep: impl Fn(&Path) -> bool,
    ) -> Vec<Hit> {
        let matcher = globset::GlobSetBuilder::new();
        let mut builder = matcher;
        for g in ignore {
            if let Ok(g) = globset::Glob::new(g) {
                builder.add(g);
            }
        }
        let set = builder.build().unwrap_or_else(|_| globset::GlobSet::empty());

        let walker = ignore::WalkBuilder::new(base)
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .parents(false)
            .follow_links(false)
            .build();

        let mut out = Vec::new();
        for entry in walker.flatten() {
            if out.len() >= cap {
                break;
            }
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if !is_file {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if set.is_match(name.as_ref()) {
                continue;
            }
            if !keep(entry.path()) {
                continue;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            out.push(Hit { path: entry.path().to_path_buf(), modified });
        }
        out
    }

    /// Matches a glob against the path relative to `base`, so `src/**` does not
    /// accidentally match a `src` folder elsewhere in the tree.
    pub fn glob_match_rel(pattern: &str, base: &Path, path: &Path) -> bool {
        let Ok(rel) = path.strip_prefix(base) else {
            return crate::permissions::glob_match(pattern, &path.to_string_lossy());
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        crate::permissions::glob_match(pattern, &rel)
    }
}

/// Path relative to the workspace root, for display in the file panel.
pub fn relative_display(ctx: &ToolContext, path: &Path) -> String {
    ctx.display(path)
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

impl ToolContext {
    /// True when a bare name should be skipped during listing and walking.
    pub fn is_ignored(&self, name: &str) -> bool {
        self.ignore.iter().any(|g| {
            g == name || crate::permissions::glob_match(g, name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_is_one_based_and_aligned() {
        let out = number_lines("a\nb\nc\n", 1, 10);
        assert_eq!(out, "1\ta\n2\tb\n3\tc\n");
    }

    #[test]
    fn numbering_honours_offset_and_limit() {
        let out = number_lines("a\nb\nc\nd\n", 2, 2);
        assert_eq!(out, "2\tb\n3\tc\n");
    }

    #[test]
    fn gutter_width_follows_the_widest_shown_number() {
        let text = (1..=2000).map(|i| format!("line{i}\n")).collect::<String>();
        // Reading the first three lines of a 2000 line file keeps a 1-wide gutter.
        assert!(number_lines(&text, 1, 3).starts_with("1\tline1"));
        // Reading near the end needs a 4-wide gutter.
        assert!(number_lines(&text, 1000, 3).starts_with("1000\t"));
    }

    #[test]
    fn glob_matches_relative_paths() {
        let base = Path::new("/w");
        assert!(walk::glob_match_rel("src/**/*.rs", base, Path::new("/w/src/a/b.rs")));
        assert!(!walk::glob_match_rel("src/**/*.rs", base, Path::new("/w/lib/a.rs")));
    }

    #[test]
    fn human_bytes_reads_naturally() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
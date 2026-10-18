use std::sync::atomic::{AtomicU64, Ordering};

use rand::Rng;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Short, sortable, collision-resistant id: `<prefix>_<base36 time><counter><rand>`.
pub fn new_id(prefix: &str) -> String {
    let t = chrono::Utc::now().timestamp_millis() as u64;
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let r: u64 = rand::rng().random();
    format!("{prefix}_{t:x}{c:04x}{r:016x}")
}

/// Truncates text to `max` characters on a char boundary, appending an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Derives a short session title from the first user message: strips fenced
/// code blocks and markdown noise, then takes the first meaningful line.
pub fn title_from(text: &str) -> String {
    let mut body = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || t.is_empty() {
            continue;
        }
        body.push_str(t);
        body.push(' ');
        if body.len() > 160 {
            break;
        }
    }
    let cleaned = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let title = truncate(&cleaned, 60);
    if title.is_empty() { "New chat".into() } else { title }
}

/// Converts a glob-ish ignore list into line endings `git check-ignore` accepts.
pub fn to_gitignore_pattern(glob: &str) -> String {
    let g = glob.trim();
    if g.is_empty() {
        return String::new();
    }
    if g.contains('/') {
        format!("/{g}")
    } else {
        g.to_string()
    }
}

/// Number of display columns, counting wide CJK characters as two.
pub fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF |
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF |
        0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE6F |
        0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F |
        0x1F900..=0x1F9FF | 0x20000..=0x2FFFD | 0x30000..=0x3FFFD)
}

/// Pads `s` to `width` display columns.
pub fn pad(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

/// Formats a token count compactly: 1234 -> "1.2k".
pub fn compact_count(n: i64) -> String {
    let a = n.abs() as f64;
    let s = if a >= 1_000_000.0 {
        format!("{:.1}m", a / 1_000_000.0)
    } else if a >= 1_000.0 {
        format!("{:.1}k", a / 1_000.0)
    } else {
        n.to_string()
    };
    if n < 0 { format!("-{s}") } else { s }
}

/// Standard base64 with padding. Used to inline images into request bodies.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Extracts the payload of a `data:<mime>;base64,<data>` url.
pub fn data_url_payload(data_url: &str) -> Option<&str> {
    data_url.split_once("base64,").map(|(_, b)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn data_urls_are_split() {
        assert_eq!(
            data_url_payload("data:image/png;base64,AAAA"),
            Some("AAAA")
        );
        assert_eq!(data_url_payload("not-a-data-url"), None);
    }

    #[test]
    fn ids_are_unique_and_prefixed() {
        let a = new_id("msg");
        let b = new_id("msg");
        assert!(a.starts_with("msg_") && b.starts_with("msg_"));
        assert_ne!(a, b);
    }

    #[test]
    fn titles_skip_code_fences() {
        let t = title_from("Fix the parser\n```rust\nfn main(){}\n```\nPlease");
        assert_eq!(t, "Fix the parser Please");
    }

    #[test]
    fn titles_fall_back_when_empty() {
        assert_eq!(title_from("```\ncode\n```"), "New chat");
        assert_eq!(title_from("   "), "New chat");
    }

    #[test]
    fn truncates_on_char_boundary() {
        // Each of these is 3 bytes; cutting at 4 must not panic mid-character.
        assert_eq!(truncate("日本語テスト", 4), "日本語テ…");
    }

    #[test]
    fn wide_chars_count_as_two_columns() {
        assert_eq!(display_width("ab"), 2);
        assert_eq!(display_width("日本"), 4);
        assert_eq!(pad("日本", 6).chars().count(), 4);
    }

    #[test]
    fn compact_counts() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1500), "1.5k");
        assert_eq!(compact_count(2_400_000), "2.4m");
    }

    #[test]
    fn anchored_ignores_only_for_paths() {
        assert_eq!(to_gitignore_pattern("node_modules"), "node_modules");
        assert_eq!(to_gitignore_pattern("src/generated"), "/src/generated");
    }
}

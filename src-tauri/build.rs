use std::path::Path;

fn main() {
    tauri_build::build();
    stage_webview_loader();
}

/// The `windows-gnu` toolchain links `WebView2Loader.dll` dynamically, and it
/// is only staged into a release bundle. Without this, `cargo run`, `cargo test`
/// and the test harness all fail at load time with STATUS_DLL_NOT_FOUND, so the
/// DLL is copied next to the binaries cargo produces.
fn stage_webview_loader() {
    let Ok(out_dir) = std::env::var("OUT_DIR") else { return };
    let out = Path::new(&out_dir);
    // .../target/<profile>/build/<crate>-<hash>/out
    let Some(build_root) = out.ancestors().nth(2) else { return };
    let Some(profile_dir) = build_root.parent() else { return };
    let Ok(entries) = std::fs::read_dir(build_root) else { return };

    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(|n| n.starts_with("webview2-com-sys-")) {
            continue;
        }
        let src = entry.path().join("out").join("x64").join("WebView2Loader.dll");
        if !src.is_file() {
            continue;
        }
        // Binaries land in the profile dir; test harnesses land in `deps/`.
        // Both need a copy: DLL lookup checks the exe's own directory first and
        // only then falls back to PATH.
        for dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
            let dest = dir.join("WebView2Loader.dll");
            if is_stale(&src, &dest) {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::copy(&src, &dest);
            }
        }
        return;
    }
}

fn is_stale(src: &Path, dest: &Path) -> bool {
    match (std::fs::metadata(src), std::fs::metadata(dest)) {
        (Ok(s), Ok(d)) => {
            s.len() != d.len()
                || s.modified().ok() != d.modified().ok()
        }
        _ => true,
    }
}

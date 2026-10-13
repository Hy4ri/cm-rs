//! Secure native download helpers for the embedded CEF browser.
//!
//! Downloads are placed in the application's XDG cache directory under
//! `cm-rs/browser/downloads/<download_id>/<sanitized_filename>`.
//! Authentication cookies and redirects are preserved because the transfer
//! is handled natively by Chromium's download machinery.

use crate::browser_types::{BrowserDownload, DownloadStatus};
use std::path::{Path, PathBuf};

/// Base directory for downloads inside XDG cache.
pub fn get_download_base_dir() -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    base.join("cm-rs").join("browser").join("downloads")
}

/// Sanitizes a suggested filename so it cannot traverse directories or
/// contain forbidden path separators. Returns a single safe filename component.
pub fn sanitize_filename(suggested: &str) -> String {
    let clean = suggested.trim();
    if clean.is_empty() {
        return "download.bin".to_string();
    }

    // Extract file name component only (strip any directory slashes)
    let p = Path::new(clean);
    let name = match p.file_name() {
        Some(f) => f.to_string_lossy().to_string(),
        None => "download.bin".to_string(),
    };

    // Filter out path traversal indicators and control characters
    let filtered: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\' && *c != '\0')
        .collect();

    let final_name = filtered.trim_matches(|c| c == '.' || c == ' ');
    if final_name.is_empty() {
        "download.bin".to_string()
    } else {
        final_name.to_string()
    }
}

/// Computes the isolated, unique download target path for an active download.
pub fn target_download_path(id: u32, suggested_name: &str) -> std::io::Result<PathBuf> {
    #[cfg(unix)]
    use std::os::unix::fs::DirBuilderExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let base = get_download_base_dir();
    std::fs::create_dir_all(&base)?;
    loop {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("{}-{stamp}-{id}-{sequence}", std::process::id()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir.join(sanitize_filename(suggested_name))),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Updates or records an item in the download list.
pub fn upsert_download_record(
    downloads: &mut Vec<BrowserDownload>,
    id: u32,
    name: String,
    path: PathBuf,
    received_bytes: i64,
    total_bytes: i64,
    status: DownloadStatus,
) {
    if let Some(pos) = downloads.iter().position(|d| d.id == id) {
        downloads[pos].received_bytes = received_bytes;
        downloads[pos].total_bytes = total_bytes;
        downloads[pos].status = status;
        if !name.is_empty() {
            downloads[pos].name = name;
        }
        if !path.as_os_str().is_empty() {
            downloads[pos].path = path;
        }
    } else {
        downloads.push(BrowserDownload {
            id,
            name,
            path,
            received_bytes,
            total_bytes,
            status,
        });
    }
}

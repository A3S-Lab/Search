//! Automatic obscura detection and installation.
//!
//! Obscura binaries are downloaded as plain executables from GitHub releases
//! (no zip extraction needed).
//!
//! Supported platforms:
//! - Linux x86_64
//! - Linux aarch64
//! - macOS x86_64
//! - macOS aarch64
//!
//! Downloaded binaries are cached in `~/.a3s/obscura/<tag>/obscura`.

use std::path::PathBuf;
use std::sync::Arc;

use tracing::{debug, info};

use crate::{Result, SearchError};

/// Progress phase during browser download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadPhase {
    FetchingVersionInfo,
    Downloading,
    Extracting,
    Completed,
    Failed,
}

/// Progress information for browser download.
#[derive(Clone, Debug)]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
}

/// Callback type for progress updates.
pub type DownloadProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync + 'static>;

/// GitHub API endpoint for the latest obscura release.
const OBSCURA_RELEASES_API: &str =
    "https://api.github.com/repos/h4ckf0r0day/obscura/releases/latest";

/// Returns the platform suffix used in obscura release asset names.
///
/// Asset names follow the pattern `obscura-<platform>`, e.g.
/// `obscura-x86_64-linux` or `obscura-aarch64-macos`.
fn platform_id() -> Result<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok("x86_64-linux");

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Ok("aarch64-linux");

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Ok("x86_64-macos");

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok("aarch64-macos");

    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    Err(SearchError::Browser(
        "Obscura does not provide binaries for this platform. \
         Supported platforms: Linux x86_64/aarch64, macOS x86_64/aarch64."
            .to_string(),
    ))
}

/// Base directory for cached obscura downloads.
fn cache_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .map_err(|_| SearchError::Browser("Cannot determine home directory".to_string()))?;
    Ok(home.join(".a3s").join("obscura"))
}

/// Detect an existing obscura installation.
///
/// Checks:
/// 1. `OBSCURA` environment variable
/// 2. `obscura` command in PATH
///
/// Returns `Some(path)` if found, `None` otherwise.
pub fn detect_obscura() -> Option<PathBuf> {
    // 1. Check OBSCURA env var
    if let Ok(path) = std::env::var("OBSCURA") {
        let p = PathBuf::from(&path);
        if p.exists() {
            debug!("Obscura found via OBSCURA env var: {}", path);
            return Some(p);
        }
    }

    // 2. Check PATH
    if let Ok(path) = which::which("obscura") {
        debug!("Obscura found in PATH: {}", path.display());
        return Some(path);
    }

    None
}

/// Look for a previously downloaded obscura in the cache directory.
fn find_cached_obscura() -> Result<PathBuf> {
    let base = cache_dir()?;
    if !base.exists() {
        return Err(SearchError::Browser("No cached Obscura found".to_string()));
    }

    // Collect version directories, newest first
    let mut versions: Vec<_> = std::fs::read_dir(&base)
        .map_err(|e| SearchError::Browser(format!("Failed to read cache dir: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    versions.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    for version_dir in versions {
        let exe_path = version_dir.path().join("obscura");
        if exe_path.exists() {
            return Ok(exe_path);
        }
    }

    Err(SearchError::Browser("No cached Obscura found".to_string()))
}

/// Ensure obscura is available, downloading it if necessary.
///
/// 1. If obscura is found via `OBSCURA` env var or PATH, returns that path.
/// 2. If a cached download exists in `~/.a3s/obscura/`, returns that path.
/// 3. Otherwise, downloads the latest release from GitHub and caches it.
pub async fn ensure_obscura() -> Result<PathBuf> {
    if let Some(path) = detect_obscura() {
        info!("Using system Obscura: {}", path.display());
        return Ok(path);
    }

    if let Ok(path) = find_cached_obscura() {
        info!("Using cached Obscura: {}", path.display());
        return Ok(path);
    }

    info!("Obscura not found, downloading latest release...");
    download_obscura().await
}

/// Ensure obscura is available with progress reporting.
pub async fn ensure_obscura_with_progress(
    progress_callback: DownloadProgressCallback,
) -> Result<PathBuf> {
    if let Some(path) = detect_obscura() {
        info!("Using system Obscura: {}", path.display());
        progress_callback(DownloadProgress {
            phase: DownloadPhase::Completed,
            downloaded_bytes: 0,
            total_bytes: None,
            message: format!("Using system Obscura: {}", path.display()),
        });
        return Ok(path);
    }

    if let Ok(path) = find_cached_obscura() {
        info!("Using cached Obscura: {}", path.display());
        progress_callback(DownloadProgress {
            phase: DownloadPhase::Completed,
            downloaded_bytes: 0,
            total_bytes: None,
            message: format!("Using cached Obscura: {}", path.display()),
        });
        return Ok(path);
    }

    info!("Obscura not found, downloading latest release...");
    download_obscura_with_progress(progress_callback).await
}

/// Download the latest obscura binary from GitHub releases with progress reporting.
async fn download_obscura_with_progress(
    progress_callback: DownloadProgressCallback,
) -> Result<PathBuf> {
    use futures::StreamExt;

    let platform = platform_id()?;
    let asset_name = format!("obscura-{}", platform);

    // Fetch latest release metadata
    progress_callback(DownloadProgress {
        phase: DownloadPhase::FetchingVersionInfo,
        downloaded_bytes: 0,
        total_bytes: None,
        message: "Fetching Obscura release info...".to_string(),
    });

    let client = reqwest::Client::builder()
        .user_agent("a3s-search")
        .build()
        .map_err(|e| SearchError::Browser(format!("Failed to create HTTP client: {}", e)))?;

    let resp = client.get(OBSCURA_RELEASES_API).send().await.map_err(|e| {
        SearchError::Browser(format!("Failed to fetch Obscura release info: {}", e))
    })?;

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        SearchError::Browser(format!("Failed to parse Obscura release JSON: {}", e))
    })?;

    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SearchError::Browser("No tag_name in Obscura release".to_string()))?;

    let assets = body
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| SearchError::Browser("No assets in Obscura release".to_string()))?;

    let download_url = assets
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset_name.as_str()))
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            SearchError::Browser(format!(
                "No Obscura binary for platform '{}' in release '{}'",
                platform, tag
            ))
        })?
        .to_string();

    // Prepare cache directory
    let version_dir = cache_dir()?.join(tag);
    std::fs::create_dir_all(&version_dir).map_err(|e| {
        SearchError::Browser(format!(
            "Failed to create Obscura cache directory {}: {}",
            version_dir.display(),
            e
        ))
    })?;

    // Download binary with progress
    progress_callback(DownloadProgress {
        phase: DownloadPhase::Downloading,
        downloaded_bytes: 0,
        total_bytes: None,
        message: format!("Downloading Obscura {} ({})...", tag, platform),
    });

    let resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to download Obscura: {}", e)))?;

    let total_bytes = resp.content_length();
    let mut downloaded_bytes = 0u64;
    let mut all_bytes = Vec::new();

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            SearchError::Browser(format!("Failed to read Obscura download chunk: {}", e))
        })?;
        downloaded_bytes += chunk.len() as u64;
        all_bytes.extend_from_slice(&chunk);

        let percent = total_bytes.map(|total| (downloaded_bytes as f64 / total as f64) * 100.0);

        progress_callback(DownloadProgress {
            phase: DownloadPhase::Downloading,
            downloaded_bytes,
            total_bytes,
            message: format!(
                "Downloading... {:.1} MB / {:.1} MB ({:.1}%)",
                downloaded_bytes as f64 / 1_048_576.0,
                total_bytes.map(|t| t as f64 / 1_048_576.0).unwrap_or(0.0),
                percent.unwrap_or(0.0)
            ),
        });
    }

    progress_callback(DownloadProgress {
        phase: DownloadPhase::Extracting,
        downloaded_bytes,
        total_bytes: Some(downloaded_bytes),
        message: "Installing...".to_string(),
    });

    let exe_path = version_dir.join("obscura");
    std::fs::write(&exe_path, &all_bytes)
        .map_err(|e| SearchError::Browser(format!("Failed to write Obscura binary: {}", e)))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| SearchError::Browser(format!("Failed to set Obscura permissions: {}", e)),
        )?;
    }

    progress_callback(DownloadProgress {
        phase: DownloadPhase::Completed,
        downloaded_bytes,
        total_bytes: Some(downloaded_bytes),
        message: format!("Obscura {} installed!", tag),
    });

    info!("Obscura installed at: {}", exe_path.display());

    Ok(exe_path)
}

/// Download the latest obscura binary from GitHub releases.
async fn download_obscura() -> Result<PathBuf> {
    let platform = platform_id()?;
    let asset_name = format!("obscura-{}", platform);

    // Fetch latest release metadata
    eprintln!("Fetching Obscura release info...");
    let client = reqwest::Client::builder()
        .user_agent("a3s-search")
        .build()
        .map_err(|e| SearchError::Browser(format!("Failed to create HTTP client: {}", e)))?;

    let resp = client.get(OBSCURA_RELEASES_API).send().await.map_err(|e| {
        SearchError::Browser(format!("Failed to fetch Obscura release info: {}", e))
    })?;

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        SearchError::Browser(format!("Failed to parse Obscura release JSON: {}", e))
    })?;

    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SearchError::Browser("No tag_name in Obscura release".to_string()))?;

    let assets = body
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| SearchError::Browser("No assets in Obscura release".to_string()))?;

    let download_url = assets
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset_name.as_str()))
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            SearchError::Browser(format!(
                "No Obscura binary for platform '{}' in release '{}'",
                platform, tag
            ))
        })?
        .to_string();

    // Prepare cache directory
    let version_dir = cache_dir()?.join(tag);
    std::fs::create_dir_all(&version_dir).map_err(|e| {
        SearchError::Browser(format!(
            "Failed to create Obscura cache directory {}: {}",
            version_dir.display(),
            e
        ))
    })?;

    // Download binary
    eprintln!("Downloading Obscura {} ({})...", tag, platform);
    let bytes = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to download Obscura: {}", e)))?
        .bytes()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to read Obscura download: {}", e)))?;

    eprintln!(
        "Downloaded {:.1} MB, installing...",
        bytes.len() as f64 / 1_048_576.0
    );

    let exe_path = version_dir.join("obscura");
    std::fs::write(&exe_path, &bytes)
        .map_err(|e| SearchError::Browser(format!("Failed to write Obscura binary: {}", e)))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| SearchError::Browser(format!("Failed to set Obscura permissions: {}", e)),
        )?;
    }

    eprintln!("Obscura {} installed at {}", tag, exe_path.display());
    info!("Obscura installed at: {}", exe_path.display());

    Ok(exe_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_id() {
        let result = platform_id();
        // On supported platforms this must succeed; on others it's an error
        #[cfg(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
        ))]
        {
            assert!(result.is_ok());
            let id = result.unwrap();
            assert!(
                [
                    "x86_64-linux",
                    "aarch64-linux",
                    "x86_64-macos",
                    "aarch64-macos",
                ]
                .contains(&id),
                "Unexpected platform id: {}",
                id
            );
        }
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
        )))]
        {
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_asset_name_format() {
        let platform = "x86_64-linux";
        let asset_name = format!("obscura-{}", platform);
        assert_eq!(asset_name, "obscura-x86_64-linux");
    }

    #[test]
    fn test_cache_dir_structure() {
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/tmp/test_obs_cache_home");
        let dir = cache_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/test_obs_cache_home/.a3s/obscura"));
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_find_cached_obscura_no_cache() {
        std::env::set_var("HOME", "/tmp/a3s_obs_nonexistent_home_xyz");
        let result = find_cached_obscura();
        assert!(result.is_err());
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_detect_obscura_nonexistent_env_path() {
        std::env::set_var("OBSCURA", "/nonexistent/obscura/binary");
        let result = detect_obscura();
        assert!(result.is_none());
        std::env::remove_var("OBSCURA");
    }

    #[test]
    fn test_find_cached_obscura_empty_dir() {
        let tmp = std::env::temp_dir().join("a3s_obs_test_empty_cache");
        std::fs::create_dir_all(&tmp).ok();

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.to_str().unwrap());
        let result = find_cached_obscura();
        assert!(result.is_err());

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_cached_obscura_with_binary() {
        let tmp = std::env::temp_dir().join("a3s_obs_test_with_binary");
        let version_dir = tmp.join(".a3s").join("obscura").join("v0.1.0");
        std::fs::create_dir_all(&version_dir).ok();

        let fake_binary = version_dir.join("obscura");
        std::fs::write(&fake_binary, b"fake").ok();

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.to_str().unwrap());
        let result = find_cached_obscura();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), fake_binary);

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_releases_api_url_is_valid() {
        assert!(OBSCURA_RELEASES_API.starts_with("https://"));
        assert!(OBSCURA_RELEASES_API.contains("h4ckf0r0day/obscura"));
    }
}

//! Automatic Lightpanda detection and installation.
//!
//! Only available when the `lightpanda` Cargo feature is enabled.
//! Lightpanda binaries are downloaded as plain executables from GitHub releases
//! (no zip extraction needed, unlike Chrome for Testing).
//!
//! Supported platforms:
//! - Linux x86_64
//! - Linux aarch64
//! - macOS x86_64
//! - macOS aarch64
//!
//! Downloaded binaries are cached in `~/.a3s/lightpanda/<tag>/lightpanda`.

use std::path::PathBuf;

use tracing::{debug, info};

use crate::{Result, SearchError};

/// GitHub API endpoint for the latest Lightpanda release.
const LIGHTPANDA_RELEASES_API: &str =
    "https://api.github.com/repos/lightpanda-io/browser/releases/latest";

/// Returns the platform suffix used in Lightpanda release asset names.
///
/// Asset names follow the pattern `lightpanda-<platform>`, e.g.
/// `lightpanda-x86_64-linux` or `lightpanda-aarch64-macos`.
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
        "Lightpanda does not provide binaries for this platform. \
         Supported platforms: Linux x86_64/aarch64, macOS x86_64/aarch64."
            .to_string(),
    ))
}

/// Base directory for cached Lightpanda downloads.
pub(crate) fn managed_cache_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .map_err(|_| SearchError::Browser("Cannot determine home directory".to_string()))?;
    Ok(home.join(".a3s").join("lightpanda"))
}

/// Detect an existing Lightpanda installation.
///
/// Checks:
/// 1. `LIGHTPANDA` environment variable
/// 2. `lightpanda` command in PATH
///
/// Returns `Some(path)` if found, `None` otherwise.
pub fn detect_lightpanda() -> Option<PathBuf> {
    // 1. Check LIGHTPANDA env var
    if let Ok(path) = std::env::var("LIGHTPANDA") {
        let p = PathBuf::from(&path);
        if p.exists() {
            debug!("Lightpanda found via LIGHTPANDA env var: {}", path);
            return Some(p);
        }
    }

    // 2. Check PATH
    if let Ok(path) = which::which("lightpanda") {
        debug!("Lightpanda found in PATH: {}", path.display());
        return Some(path);
    }

    None
}

/// Look for a previously downloaded Lightpanda in the cache directory.
pub(crate) fn find_managed_lightpanda() -> Result<PathBuf> {
    let base = managed_cache_dir()?;
    if !base.exists() {
        return Err(SearchError::Browser(
            "No cached Lightpanda found".to_string(),
        ));
    }

    // Collect version directories, newest first
    let mut versions: Vec<_> = std::fs::read_dir(&base)
        .map_err(|e| SearchError::Browser(format!("Failed to read cache dir: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    versions.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    for version_dir in versions {
        let exe_path = version_dir.path().join("lightpanda");
        if exe_path.exists() {
            return Ok(exe_path);
        }
    }

    Err(SearchError::Browser(
        "No cached Lightpanda found".to_string(),
    ))
}

/// Ensure Lightpanda is available, downloading it if necessary.
///
/// 1. If Lightpanda is found via `LIGHTPANDA` env var or PATH, returns that path.
/// 2. If a cached download exists in `~/.a3s/lightpanda/`, returns that path.
/// 3. Otherwise, downloads the latest release from GitHub and caches it.
pub async fn ensure_lightpanda() -> Result<PathBuf> {
    if let Some(path) = detect_lightpanda() {
        info!("Using system Lightpanda: {}", path.display());
        return Ok(path);
    }

    if let Ok(path) = find_managed_lightpanda() {
        info!("Using cached Lightpanda: {}", path.display());
        return Ok(path);
    }

    info!("Lightpanda not found, downloading latest release...");
    download_latest_lightpanda().await
}

/// Download the latest Lightpanda binary from GitHub releases.
pub async fn download_latest_lightpanda() -> Result<PathBuf> {
    let platform = platform_id()?;
    let asset_name = format!("lightpanda-{}", platform);

    // Fetch latest release metadata
    eprintln!("Fetching Lightpanda release info...");
    let client = reqwest::Client::builder()
        .user_agent("a3s-search")
        .build()
        .map_err(|e| SearchError::Browser(format!("Failed to create HTTP client: {}", e)))?;

    let resp = client
        .get(LIGHTPANDA_RELEASES_API)
        .send()
        .await
        .map_err(|e| {
            SearchError::Browser(format!("Failed to fetch Lightpanda release info: {}", e))
        })?;

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        SearchError::Browser(format!("Failed to parse Lightpanda release JSON: {}", e))
    })?;

    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SearchError::Browser("No tag_name in Lightpanda release".to_string()))?;

    let assets = body
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| SearchError::Browser("No assets in Lightpanda release".to_string()))?;

    let download_url = assets
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset_name.as_str()))
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            SearchError::Browser(format!(
                "No Lightpanda binary for platform '{}' in release '{}'",
                platform, tag
            ))
        })?
        .to_string();

    // Prepare cache directory
    let version_dir = managed_cache_dir()?.join(tag);
    std::fs::create_dir_all(&version_dir).map_err(|e| {
        SearchError::Browser(format!(
            "Failed to create Lightpanda cache directory {}: {}",
            version_dir.display(),
            e
        ))
    })?;

    // Download binary
    eprintln!("Downloading Lightpanda {} ({})...", tag, platform);
    let bytes = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to download Lightpanda: {}", e)))?
        .bytes()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to read Lightpanda download: {}", e)))?;

    eprintln!(
        "Downloaded {:.1} MB, installing...",
        bytes.len() as f64 / 1_048_576.0
    );

    let exe_path = version_dir.join("lightpanda");
    std::fs::write(&exe_path, &bytes)
        .map_err(|e| SearchError::Browser(format!("Failed to write Lightpanda binary: {}", e)))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| SearchError::Browser(format!("Failed to set Lightpanda permissions: {}", e)),
        )?;
    }

    eprintln!("Lightpanda {} installed at {}", tag, exe_path.display());
    info!("Lightpanda installed at: {}", exe_path.display());

    Ok(exe_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{env_lock, EnvVarGuard};

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
                    "aarch64-macos"
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
        // Verify asset name construction matches GitHub release naming
        let platform = "x86_64-linux";
        let asset_name = format!("lightpanda-{}", platform);
        assert_eq!(asset_name, "lightpanda-x86_64-linux");
    }

    #[test]
    fn test_cache_dir_structure() {
        let _lock = env_lock();
        let _home = EnvVarGuard::set("HOME", "/tmp/test_lp_cache_home");
        let dir = managed_cache_dir().unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/tmp/test_lp_cache_home/.a3s/lightpanda")
        );
    }

    #[test]
    fn test_find_cached_lightpanda_no_cache() {
        let _lock = env_lock();
        let _home = EnvVarGuard::set("HOME", "/tmp/a3s_lp_nonexistent_home_xyz");
        let result = find_managed_lightpanda();
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_lightpanda_nonexistent_env_path() {
        let _lock = env_lock();
        // Set LIGHTPANDA to a non-existent path — should not return it
        let _lightpanda = EnvVarGuard::set("LIGHTPANDA", "/nonexistent/lightpanda/binary");
        let result = detect_lightpanda();
        if let Some(ref path) = result {
            assert_ne!(
                path,
                &PathBuf::from("/nonexistent/lightpanda/binary"),
                "Should not return non-existent LIGHTPANDA env path"
            );
        }
    }

    #[test]
    fn test_find_cached_lightpanda_empty_dir() {
        let _lock = env_lock();
        let tmp = std::env::temp_dir().join("a3s_lp_test_empty_cache");
        let cache = tmp.join(".a3s").join("lightpanda");
        std::fs::create_dir_all(&cache).ok();

        let _home = EnvVarGuard::set("HOME", tmp.as_os_str());
        let result = find_managed_lightpanda();
        assert!(result.is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_cached_lightpanda_version_dir_no_binary() {
        let _lock = env_lock();
        let tmp = std::env::temp_dir().join("a3s_lp_test_no_binary");
        let version_dir = tmp
            .join(".a3s")
            .join("lightpanda")
            .join("nightly-2024-01-01");
        std::fs::create_dir_all(&version_dir).ok();

        let _home = EnvVarGuard::set("HOME", tmp.as_os_str());
        let result = find_managed_lightpanda();
        assert!(result.is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_cached_lightpanda_with_binary() {
        let _lock = env_lock();
        let tmp = std::env::temp_dir().join("a3s_lp_test_with_binary");
        let version_dir = tmp
            .join(".a3s")
            .join("lightpanda")
            .join("nightly-2024-01-01");
        std::fs::create_dir_all(&version_dir).ok();

        // Create a fake binary
        let fake_binary = version_dir.join("lightpanda");
        std::fs::write(&fake_binary, b"fake").ok();

        let _home = EnvVarGuard::set("HOME", tmp.as_os_str());
        let result = find_managed_lightpanda();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), fake_binary);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_releases_api_url_is_valid() {
        assert!(LIGHTPANDA_RELEASES_API.starts_with("https://"));
        assert!(LIGHTPANDA_RELEASES_API.contains("lightpanda-io/browser"));
    }
}

//! Automatic Chrome/Chromium detection and installation.
//!
//! When the `headless` feature is enabled, this module provides utilities to
//! detect an existing Chrome/Chromium installation or automatically download
//! Chrome for Testing from Google's official CDN.
//!
//! Downloaded binaries are cached in `~/.a3s/chromium/<version>/`.

use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::{Result, SearchError};

/// JSON API endpoint for Chrome for Testing stable versions.
const CHROME_VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// Well-known Chrome/Chromium executable paths per platform.
#[cfg(target_os = "macos")]
const KNOWN_PATHS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
];

#[cfg(all(unix, not(target_os = "macos")))]
const KNOWN_PATHS: &[&str] = &[
    "/opt/google/chrome/chrome",
    "/opt/chromium.org/chromium/chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/snap/bin/chromium",
];

/// Well-known command names to search in PATH.
const KNOWN_COMMANDS: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
];

/// Returns the platform identifier for Chrome for Testing downloads.
fn platform_id() -> Result<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Ok("mac-arm64")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Ok("mac-x64")
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Ok("linux64")
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
    )))]
    {
        Err(SearchError::Browser(
            "Unsupported platform for automatic Chrome download".to_string(),
        ))
    }
}

/// Returns the relative path to the Chrome executable inside the extracted zip.
#[cfg(target_os = "macos")]
fn chrome_executable_in_zip(platform: &str) -> String {
    format!(
        "chrome-{}/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        platform
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn chrome_executable_in_zip(platform: &str) -> String {
    format!("chrome-{}/chrome", platform)
}

/// Base directory for cached Chrome downloads.
fn cache_dir() -> Result<PathBuf> {
    let home = dirs_path()?;
    Ok(home.join(".a3s").join("chromium"))
}

/// Returns the user's home directory.
fn dirs_path() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| SearchError::Browser("Cannot determine home directory".to_string()))
}

/// Detect an existing Chrome/Chromium installation on the system.
///
/// Checks:
/// 1. `CHROME` environment variable
/// 2. Well-known command names in PATH
/// 3. Well-known filesystem paths
///
/// Returns `Some(path)` if found, `None` otherwise.
pub fn detect_chrome() -> Option<PathBuf> {
    // 1. Check CHROME env var
    if let Ok(path) = std::env::var("CHROME") {
        let p = PathBuf::from(&path);
        if p.exists() {
            debug!("Chrome found via CHROME env var: {}", path);
            return Some(p);
        }
    }

    // 2. Check well-known commands in PATH
    for cmd in KNOWN_COMMANDS {
        if let Ok(path) = which::which(cmd) {
            debug!("Chrome found in PATH: {}", path.display());
            return Some(path);
        }
    }

    // 3. Check well-known filesystem paths
    for path_str in KNOWN_PATHS {
        let p = Path::new(path_str);
        if p.exists() {
            debug!("Chrome found at known path: {}", path_str);
            return Some(p.to_path_buf());
        }
    }

    None
}

/// Ensure Chrome is available, downloading it if necessary.
///
/// 1. If Chrome is already installed on the system, returns its path.
/// 2. If a cached download exists in `~/.a3s/chromium/`, returns that path.
/// 3. Otherwise, downloads Chrome for Testing and caches it.
///
/// Returns the path to the Chrome executable.
pub async fn ensure_chrome() -> Result<PathBuf> {
    // 1. Check system installation
    if let Some(path) = detect_chrome() {
        info!("Using system Chrome: {}", path.display());
        return Ok(path);
    }

    // 2. Check cached download
    if let Ok(path) = find_cached_chrome() {
        info!("Using cached Chrome: {}", path.display());
        return Ok(path);
    }

    // 3. Download Chrome for Testing
    info!("No Chrome installation found, downloading Chrome for Testing...");
    download_chrome().await
}

/// Look for a previously downloaded Chrome in the cache directory.
fn find_cached_chrome() -> Result<PathBuf> {
    let base = cache_dir()?;
    if !base.exists() {
        return Err(SearchError::Browser("No cached Chrome found".to_string()));
    }

    // Find the latest version directory
    let mut versions: Vec<_> = std::fs::read_dir(&base)
        .map_err(|e| SearchError::Browser(format!("Failed to read cache dir: {}", e)))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();

    // Sort by name descending (latest version first)
    versions.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    let platform = platform_id()?;
    for version_dir in versions {
        let exe_path = version_dir.path().join(chrome_executable_in_zip(platform));
        if exe_path.exists() {
            return Ok(exe_path);
        }
    }

    Err(SearchError::Browser("No cached Chrome found".to_string()))
}

/// Download Chrome for Testing from Google's official CDN.
///
/// Downloads the stable version for the current platform and extracts it
/// to `~/.a3s/chromium/<version>/`.
async fn download_chrome() -> Result<PathBuf> {
    let platform = platform_id()?;

    // Fetch version metadata
    eprintln!("Fetching Chrome for Testing version info...");
    let client = reqwest::Client::new();
    let resp = client
        .get(CHROME_VERSIONS_URL)
        .send()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to fetch Chrome versions: {}", e)))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to parse Chrome versions JSON: {}", e)))?;

    // Extract stable channel info
    let stable = body
        .get("channels")
        .and_then(|c| c.get("Stable"))
        .ok_or_else(|| SearchError::Browser("No Stable channel in Chrome versions".to_string()))?;

    let version = stable
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SearchError::Browser("No version in Stable channel".to_string()))?;

    // Find download URL for our platform
    let downloads = stable
        .get("downloads")
        .and_then(|d| d.get("chrome"))
        .and_then(|c| c.as_array())
        .ok_or_else(|| SearchError::Browser("No chrome downloads in Stable channel".to_string()))?;

    let download_url = downloads
        .iter()
        .find(|d| d.get("platform").and_then(|p| p.as_str()) == Some(platform))
        .and_then(|d| d.get("url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            SearchError::Browser(format!(
                "No Chrome download available for platform '{}'",
                platform
            ))
        })?;

    // Prepare cache directory
    let version_dir = cache_dir()?.join(version);
    std::fs::create_dir_all(&version_dir).map_err(|e| {
        SearchError::Browser(format!(
            "Failed to create cache directory {}: {}",
            version_dir.display(),
            e
        ))
    })?;

    // Download the zip
    eprintln!(
        "Downloading Chrome for Testing v{} ({})...",
        version, platform
    );
    let zip_bytes = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to download Chrome: {}", e)))?
        .bytes()
        .await
        .map_err(|e| SearchError::Browser(format!("Failed to read Chrome download: {}", e)))?;

    eprintln!(
        "Downloaded {:.1} MB, extracting...",
        zip_bytes.len() as f64 / 1_048_576.0
    );

    // Extract the zip
    extract_zip(&zip_bytes, &version_dir)?;

    // Find the executable
    let exe_path = version_dir.join(chrome_executable_in_zip(platform));

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if exe_path.exists() {
            let mut perms = std::fs::metadata(&exe_path)
                .map_err(|e| {
                    SearchError::Browser(format!("Failed to read Chrome permissions: {}", e))
                })?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe_path, perms).map_err(|e| {
                SearchError::Browser(format!("Failed to set Chrome permissions: {}", e))
            })?;
        }
    }

    if !exe_path.exists() {
        // List what was actually extracted for debugging
        let contents: Vec<_> = std::fs::read_dir(&version_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        warn!(
            "Expected Chrome at {} but not found. Extracted contents: {:?}",
            exe_path.display(),
            contents
        );
        return Err(SearchError::Browser(format!(
            "Chrome executable not found after extraction at {}",
            exe_path.display()
        )));
    }

    eprintln!("Chrome for Testing v{} installed successfully!", version);
    info!("Chrome installed at: {}", exe_path.display());

    Ok(exe_path)
}

/// Extract a zip archive to the target directory.
fn extract_zip(zip_bytes: &[u8], target_dir: &Path) -> Result<()> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| SearchError::Browser(format!("Failed to open zip archive: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| SearchError::Browser(format!("Failed to read zip entry {}: {}", i, e)))?;

        let out_path = target_dir.join(file.mangled_name());

        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| {
                SearchError::Browser(format!(
                    "Failed to create directory {}: {}",
                    out_path.display(),
                    e
                ))
            })?;
        } else {
            // Ensure parent directory exists
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    SearchError::Browser(format!(
                        "Failed to create parent directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }

            let mut outfile = std::fs::File::create(&out_path).map_err(|e| {
                SearchError::Browser(format!(
                    "Failed to create file {}: {}",
                    out_path.display(),
                    e
                ))
            })?;

            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| {
                SearchError::Browser(format!("Failed to read zip entry: {}", e))
            })?;

            std::io::Write::write_all(&mut outfile, &buf).map_err(|e| {
                SearchError::Browser(format!(
                    "Failed to write file {}: {}",
                    out_path.display(),
                    e
                ))
            })?;

            // Preserve Unix permissions from zip
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))
                        .ok();
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_id() {
        let platform = platform_id();
        assert!(platform.is_ok());
        let id = platform.unwrap();
        assert!(
            ["mac-arm64", "mac-x64", "linux64"].contains(&id),
            "Unexpected platform: {}",
            id
        );
    }

    #[test]
    fn test_chrome_executable_in_zip_format() {
        let path = chrome_executable_in_zip("mac-arm64");
        assert!(path.contains("chrome-mac-arm64"));
        assert!(!path.is_empty());
    }

    #[test]
    fn test_cache_dir() {
        let dir = cache_dir();
        assert!(dir.is_ok());
        let path = dir.unwrap();
        assert!(path.to_string_lossy().contains(".a3s/chromium"));
    }

    #[test]
    fn test_detect_chrome_returns_option() {
        // This test just verifies the function runs without panic.
        // On CI without Chrome, it returns None; on dev machines, Some.
        let result = detect_chrome();
        if let Some(ref path) = result {
            assert!(path.exists());
        }
    }

    #[test]
    fn test_detect_chrome_respects_env_var() {
        // Set CHROME to a non-existent path — should not return it
        std::env::set_var("CHROME", "/nonexistent/chrome/binary");
        let result = detect_chrome();
        // Should not return the non-existent path
        if let Some(ref path) = result {
            assert_ne!(
                path,
                &PathBuf::from("/nonexistent/chrome/binary"),
                "Should not return non-existent CHROME env path"
            );
        }
        std::env::remove_var("CHROME");
    }

    #[test]
    fn test_find_cached_chrome_no_cache() {
        // With no cache directory, should return error
        std::env::set_var("HOME", "/tmp/a3s_test_nonexistent_home");
        let result = find_cached_chrome();
        assert!(result.is_err());
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_known_paths_not_empty() {
        assert!(!KNOWN_PATHS.is_empty());
    }

    #[test]
    fn test_known_commands_not_empty() {
        assert!(!KNOWN_COMMANDS.is_empty());
    }

    #[test]
    fn test_extract_zip_invalid_data() {
        let result = extract_zip(b"not a zip file", Path::new("/tmp"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("zip"), "Error should mention zip: {}", err);
    }

    #[test]
    fn test_dirs_path_returns_home() {
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/tmp/test_home_dir");
        let result = dirs_path();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from("/tmp/test_home_dir"));
        // Restore
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_cache_dir_structure() {
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/tmp/test_cache_home");
        let dir = cache_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/test_cache_home/.a3s/chromium"));
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    #[test]
    fn test_chrome_executable_in_zip_linux_format() {
        let path = chrome_executable_in_zip("linux64");
        assert!(path.contains("chrome-linux64"));
    }

    #[test]
    fn test_chrome_executable_in_zip_mac_x64_format() {
        let path = chrome_executable_in_zip("mac-x64");
        assert!(path.contains("chrome-mac-x64"));
    }

    #[test]
    fn test_find_cached_chrome_empty_cache_dir() {
        // Create a temporary cache directory with no version subdirs
        let tmp = std::env::temp_dir().join("a3s_test_empty_cache");
        let cache = tmp.join(".a3s").join("chromium");
        std::fs::create_dir_all(&cache).ok();

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.to_str().unwrap());
        let result = find_cached_chrome();
        assert!(result.is_err());

        // Cleanup
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_cached_chrome_version_dir_without_executable() {
        // Create a cache directory with a version subdir but no executable
        let tmp = std::env::temp_dir().join("a3s_test_no_exe_cache");
        let version_dir = tmp.join(".a3s").join("chromium").join("130.0.6723.58");
        std::fs::create_dir_all(&version_dir).ok();

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.to_str().unwrap());
        let result = find_cached_chrome();
        assert!(result.is_err());

        // Cleanup
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn test_ensure_chrome_finds_system_chrome() {
        // If Chrome is installed on this system, ensure_chrome should find it
        if detect_chrome().is_some() {
            let result = ensure_chrome().await;
            assert!(result.is_ok());
            assert!(result.unwrap().exists());
        }
    }

    #[test]
    fn test_chrome_versions_url_is_valid() {
        assert!(CHROME_VERSIONS_URL.starts_with("https://"));
        assert!(CHROME_VERSIONS_URL.contains("chrome-for-testing"));
    }

    #[test]
    fn test_extract_zip_valid_zip() {
        // Create a minimal valid zip in memory
        use std::io::Write;
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        zip_writer.start_file("test.txt", options).unwrap();
        zip_writer.write_all(b"hello world").unwrap();
        let cursor = zip_writer.finish().unwrap();
        let zip_bytes = cursor.into_inner();

        let tmp_dir = std::env::temp_dir().join("a3s_test_extract_valid");
        std::fs::create_dir_all(&tmp_dir).ok();

        let result = extract_zip(&zip_bytes, &tmp_dir);
        assert!(result.is_ok());

        // Verify the file was extracted
        let extracted = tmp_dir.join("test.txt");
        assert!(extracted.exists());
        let content = std::fs::read_to_string(&extracted).unwrap();
        assert_eq!(content, "hello world");

        // Cleanup
        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn test_extract_zip_with_directory() {
        // Create a zip with a directory entry
        use std::io::Write;
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        zip_writer.add_directory("subdir", options).unwrap();
        zip_writer.start_file("subdir/file.txt", options).unwrap();
        zip_writer.write_all(b"nested content").unwrap();
        let cursor = zip_writer.finish().unwrap();
        let zip_bytes = cursor.into_inner();

        let tmp_dir = std::env::temp_dir().join("a3s_test_extract_dir");
        std::fs::create_dir_all(&tmp_dir).ok();

        let result = extract_zip(&zip_bytes, &tmp_dir);
        assert!(result.is_ok());

        let nested = tmp_dir.join("subdir").join("file.txt");
        assert!(nested.exists());
        let content = std::fs::read_to_string(&nested).unwrap();
        assert_eq!(content, "nested content");

        // Cleanup
        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}

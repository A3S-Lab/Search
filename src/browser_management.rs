//! Public lifecycle API for browser runtimes used by headless search engines.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Result, SearchError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedBrowser {
    Chrome,
    Lightpanda,
}

impl ManagedBrowser {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Lightpanda => "lightpanda",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserInstallSource {
    Environment,
    System,
    ManagedCache,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRuntimeStatus {
    pub browser: ManagedBrowser,
    pub available: bool,
    pub source: BrowserInstallSource,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub cache_dir: Option<PathBuf>,
    pub detail: String,
}

pub fn browser_status(browser: ManagedBrowser) -> BrowserRuntimeStatus {
    match browser {
        ManagedBrowser::Chrome => chrome_status(),
        ManagedBrowser::Lightpanda => lightpanda_status(),
    }
}

pub fn browser_statuses() -> Vec<BrowserRuntimeStatus> {
    [ManagedBrowser::Chrome, ManagedBrowser::Lightpanda]
        .into_iter()
        .map(browser_status)
        .collect()
}

pub async fn install_browser(browser: ManagedBrowser) -> Result<BrowserRuntimeStatus> {
    install_or_update_browser(browser, false).await
}

pub async fn update_browser(browser: ManagedBrowser) -> Result<BrowserRuntimeStatus> {
    install_or_update_browser(browser, true).await
}

pub async fn repair_browser(browser: ManagedBrowser) -> Result<BrowserRuntimeStatus> {
    let status = browser_status(browser);
    if status.available {
        Ok(status)
    } else {
        update_browser(browser).await
    }
}

async fn install_or_update_browser(
    browser: ManagedBrowser,
    force_latest: bool,
) -> Result<BrowserRuntimeStatus> {
    match browser {
        ManagedBrowser::Chrome => {
            if force_latest {
                crate::browser_setup::download_latest_chrome().await?;
            } else {
                crate::browser_setup::ensure_chrome().await?;
            }
        }
        ManagedBrowser::Lightpanda => {
            #[cfg(feature = "lightpanda")]
            if force_latest {
                crate::browser_setup_lp::download_latest_lightpanda().await?;
            } else {
                crate::browser_setup_lp::ensure_lightpanda().await?;
            }
            #[cfg(not(feature = "lightpanda"))]
            return Err(SearchError::Browser(
                "Lightpanda support is not compiled into this build".to_string(),
            ));
        }
    }
    let status = browser_status(browser);
    status.available.then_some(status).ok_or_else(|| {
        SearchError::Browser(format!(
            "{} installation completed without a usable executable",
            browser.as_str()
        ))
    })
}

fn chrome_status() -> BrowserRuntimeStatus {
    let cache_dir = crate::browser_setup::managed_cache_dir().ok();
    if let Some(path) = env_executable("CHROME") {
        return available_status(
            ManagedBrowser::Chrome,
            BrowserInstallSource::Environment,
            path,
            cache_dir,
        );
    }
    if let Some(path) = crate::browser_setup::detect_chrome() {
        return available_status(
            ManagedBrowser::Chrome,
            BrowserInstallSource::System,
            path,
            cache_dir,
        );
    }
    if let Ok(path) = crate::browser_setup::find_managed_chrome() {
        return available_status(
            ManagedBrowser::Chrome,
            BrowserInstallSource::ManagedCache,
            path,
            cache_dir,
        );
    }
    missing_status(ManagedBrowser::Chrome, cache_dir, "not installed")
}

fn lightpanda_status() -> BrowserRuntimeStatus {
    #[cfg(feature = "lightpanda")]
    {
        let cache_dir = crate::browser_setup_lp::managed_cache_dir().ok();
        if let Some(path) = env_executable("LIGHTPANDA") {
            return available_status(
                ManagedBrowser::Lightpanda,
                BrowserInstallSource::Environment,
                path,
                cache_dir,
            );
        }
        if let Some(path) = crate::browser_setup_lp::detect_lightpanda() {
            return available_status(
                ManagedBrowser::Lightpanda,
                BrowserInstallSource::System,
                path,
                cache_dir,
            );
        }
        if let Ok(path) = crate::browser_setup_lp::find_managed_lightpanda() {
            return available_status(
                ManagedBrowser::Lightpanda,
                BrowserInstallSource::ManagedCache,
                path,
                cache_dir,
            );
        }
        missing_status(ManagedBrowser::Lightpanda, cache_dir, "not installed")
    }
    #[cfg(not(feature = "lightpanda"))]
    {
        BrowserRuntimeStatus {
            browser: ManagedBrowser::Lightpanda,
            available: false,
            source: BrowserInstallSource::Unsupported,
            path: None,
            version: None,
            cache_dir: None,
            detail: "support is not compiled into this build".to_string(),
        }
    }
}

fn env_executable(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn available_status(
    browser: ManagedBrowser,
    source: BrowserInstallSource,
    path: PathBuf,
    cache_dir: Option<PathBuf>,
) -> BrowserRuntimeStatus {
    if !is_usable_executable(&path) {
        return BrowserRuntimeStatus {
            browser,
            available: false,
            source,
            path: Some(path),
            version: None,
            cache_dir,
            detail: "executable is not runnable; use the repair operation".to_string(),
        };
    }
    let version = (source == BrowserInstallSource::ManagedCache)
        .then(|| {
            let cache = cache_dir.as_deref()?;
            path.strip_prefix(cache)
                .ok()?
                .components()
                .next()?
                .as_os_str()
                .to_str()
                .map(str::to_string)
        })
        .flatten();
    BrowserRuntimeStatus {
        browser,
        available: true,
        source,
        detail: "ready".to_string(),
        path: Some(path),
        version,
        cache_dir,
    }
}

fn is_usable_executable(path: &std::path::Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn missing_status(
    browser: ManagedBrowser,
    cache_dir: Option<PathBuf>,
    detail: &str,
) -> BrowserRuntimeStatus {
    BrowserRuntimeStatus {
        browser,
        available: false,
        source: BrowserInstallSource::Missing,
        path: None,
        version: None,
        cache_dir,
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_ids_are_stable() {
        assert_eq!(ManagedBrowser::Chrome.as_str(), "chrome");
        assert_eq!(ManagedBrowser::Lightpanda.as_str(), "lightpanda");
    }

    #[test]
    fn missing_status_is_explicit() {
        let status = missing_status(ManagedBrowser::Chrome, None, "not installed");
        assert!(!status.available);
        assert_eq!(status.source, BrowserInstallSource::Missing);
        assert_eq!(status.path, None);
    }

    #[cfg(unix)]
    #[test]
    fn managed_status_reports_cache_version_and_executable_health() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "a3s-search-browser-management-{}",
            std::process::id()
        ));
        let cache = root.join("chromium");
        let executable = cache.join("123.4.5").join("bundle").join("chrome");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"browser").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();

        let status = available_status(
            ManagedBrowser::Chrome,
            BrowserInstallSource::ManagedCache,
            executable.clone(),
            Some(cache),
        );
        assert!(status.available);
        assert_eq!(status.version.as_deref(), Some("123.4.5"));
        assert_eq!(status.path.as_deref(), Some(executable.as_path()));

        let _ = std::fs::remove_dir_all(root);
    }
}

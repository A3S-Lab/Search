mod search;
mod types;
mod util;

use napi_derive::napi;
use util::to_napi_error;

/// Download Chrome for Testing if not already installed.
///
/// Returns the path to the Chrome executable.
#[napi]
pub async fn ensure_chrome() -> napi::Result<String> {
    a3s_search::browser_setup::ensure_chrome()
        .await
        .map(|p| p.to_string_lossy().to_string())
        .map_err(to_napi_error)
}

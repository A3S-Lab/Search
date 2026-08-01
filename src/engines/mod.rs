//! Search engine implementations.

// International engines
mod bing;
mod brave;
mod duckduckgo;
mod wikipedia;

// Chinese engines
mod bing_china;
mod so360;
mod sogou;

// Headless browser engines (require JavaScript rendering)
#[cfg(feature = "headless")]
mod baidu;
#[cfg(feature = "headless")]
mod google;

pub use bing::{Bing, BingBrowser, BingBrowserParser, BingParser};
pub use brave::{Brave, BraveBrowser, BraveBrowserParser, BraveParser};
pub use duckduckgo::{DuckDuckGo, DuckDuckGoParser};
pub use wikipedia::Wikipedia;

pub use bing_china::{BingChina, BingChinaParser};
pub use so360::{So360, So360Parser};
pub use sogou::{Sogou, SogouParser};

#[cfg(feature = "headless")]
pub use baidu::{Baidu, BaiduParser};
#[cfg(feature = "headless")]
pub use google::{Google, GoogleParser};

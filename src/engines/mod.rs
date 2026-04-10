//! Search engine implementations.

// International engines
mod bing;
mod brave;
mod duckduckgo;
mod wikipedia;

// Chinese engines
mod so360;
mod sogou;

// Headless browser engines (require JavaScript rendering via chromium or lightpanda)
#[cfg(any(feature = "chromium", feature = "lightpanda"))]
mod baidu;
#[cfg(any(feature = "chromium", feature = "lightpanda"))]
mod bing_china;
#[cfg(any(feature = "chromium", feature = "lightpanda"))]
mod google;

pub use bing::{Bing, BingParser};
pub use brave::{Brave, BraveParser};
pub use duckduckgo::{DuckDuckGo, DuckDuckGoParser};
pub use wikipedia::Wikipedia;

pub use so360::{So360, So360Parser};
pub use sogou::{Sogou, SogouParser};

#[cfg(any(feature = "chromium", feature = "lightpanda"))]
pub use baidu::{Baidu, BaiduParser};
#[cfg(any(feature = "chromium", feature = "lightpanda"))]
pub use bing_china::{BingChina, BingChinaParser};
#[cfg(any(feature = "chromium", feature = "lightpanda"))]
pub use google::{Google, GoogleParser};

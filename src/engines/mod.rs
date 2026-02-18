//! Search engine implementations.

// International engines
mod bing;
mod brave;
mod duckduckgo;
mod wikipedia;

// Chinese engines
mod so360;
mod sogou;

// Headless browser engines (require JavaScript rendering)
#[cfg(feature = "headless")]
mod baidu;
#[cfg(feature = "headless")]
mod bing_china;
#[cfg(feature = "headless")]
mod google;

pub use bing::{Bing, BingParser};
pub use brave::{Brave, BraveParser};
pub use duckduckgo::{DuckDuckGo, DuckDuckGoParser};
pub use wikipedia::Wikipedia;

pub use so360::{So360, So360Parser};
pub use sogou::{Sogou, SogouParser};

#[cfg(feature = "headless")]
pub use baidu::{Baidu, BaiduParser};
#[cfg(feature = "headless")]
pub use bing_china::{BingChina, BingChinaParser};
#[cfg(feature = "headless")]
pub use google::{Google, GoogleParser};

//! Search engine implementations.

// International engines
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

pub use brave::Brave;
pub use duckduckgo::DuckDuckGo;
pub use wikipedia::Wikipedia;

pub use so360::So360;
pub use sogou::Sogou;

#[cfg(feature = "headless")]
pub use baidu::Baidu;
#[cfg(feature = "headless")]
pub use bing_china::BingChina;
#[cfg(feature = "headless")]
pub use google::Google;

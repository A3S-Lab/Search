//! Search engine implementations.

// International engines
mod duckduckgo;
mod brave;
mod google;
mod wikipedia;

// Chinese engines
mod baidu;
mod sogou;
mod bing_china;
mod so360;

pub use duckduckgo::DuckDuckGo;
pub use brave::Brave;
pub use google::Google;
pub use wikipedia::Wikipedia;

pub use baidu::Baidu;
pub use sogou::Sogou;
pub use bing_china::BingChina;
pub use so360::So360;

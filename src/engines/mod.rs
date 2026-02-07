//! Search engine implementations.

// International engines
mod brave;
mod duckduckgo;
mod google;
mod wikipedia;

// Chinese engines
mod baidu;
mod bing_china;
mod so360;
mod sogou;

pub use brave::Brave;
pub use duckduckgo::DuckDuckGo;
pub use google::Google;
pub use wikipedia::Wikipedia;

pub use baidu::Baidu;
pub use bing_china::BingChina;
pub use so360::So360;
pub use sogou::Sogou;

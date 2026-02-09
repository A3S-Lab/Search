//! Search engine implementations.

// International engines
mod brave;
mod duckduckgo;
mod wikipedia;

// Chinese engines
mod so360;
mod sogou;

pub use brave::Brave;
pub use duckduckgo::DuckDuckGo;
pub use wikipedia::Wikipedia;

pub use so360::So360;
pub use sogou::Sogou;

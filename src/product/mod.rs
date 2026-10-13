//! Product cascade: library-owned plan execution for embedders and the CLI.
//!
//! [`crate::EngineTierPlan`] selects sources. This module runs that plan through
//! [`crate::SearchCascade`] with shared reliability controls.

mod browser;
mod run;
mod sources;

pub use browser::HeadlessBrowser;
pub use run::{execute_cascade, CascadeControls, CascadeRequest, DEFAULT_TIMEOUT_SECONDS};
pub use sources::{
    configured_engine_config, create_http_fetcher, create_provider_engine, ensure_provider_ready,
    is_config_enabled,
};

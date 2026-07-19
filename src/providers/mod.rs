//! Native third-party search provider integrations.
//!
//! Providers model authenticated JSON search APIs separately from scraping
//! engines. [`ProviderEngine`] is the reusable adapter that lets any
//! [`SearchProvider`] participate in the existing meta-search orchestration.

mod anysearch;
mod builtin;
mod credential;
mod engine;
mod http;
mod metadata;
mod normalization;
mod protocol;
mod tavily;

pub use anysearch::{AnySearchConfig, AnySearchDomain, AnySearchProvider, AnySearchSubDomain};
pub use builtin::BuiltinProvider;
pub use credential::{CredentialSource, ProviderAuthentication, ProviderReadiness};
pub use engine::ProviderEngine;
pub use http::ProviderHttpConfig;
pub use protocol::{
    ProviderCapabilities, ProviderDescriptor, ProviderReport, ProviderRequest, ProviderResponse,
    ProviderResult, SearchProvider,
};
pub use tavily::{
    TavilyAnswer, TavilyConfig, TavilyCountry, TavilyDate, TavilyProvider, TavilyRawContent,
    TavilySearchDepth, TavilyTopic,
};

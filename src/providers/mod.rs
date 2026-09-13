//! Native third-party search provider integrations.
//!
//! Every native API implements [`SearchProvider`]. [`ProviderEngine`] is the
//! only adapter into cascade, ranking, and the CLI. Vendor modules do not
//! talk to those layers.
//!
//! Required-credential JSON APIs share one transport shell. The shell executes
//! the call and keeps the credential for the life of the reply. Error
//! classification and the success body are sealed against that credential
//! before the reply is dropped, so a result cannot leave with the key. A
//! vendor module supplies only its options, request mapping, response mapping,
//! and error-code classification. Result caps use `max_results` in Rust and ACL;
//! the vendor wire field stays inside that module. Optional-auth and MCP
//! codecs stay off this shell.

mod aliyun;
mod anysearch;
mod bocha;
mod builtin;
mod credential;
mod engine;
mod firecrawl;
mod http;
mod json_api;
mod metadata;
mod normalization;
mod protocol;
mod tavily;
mod tencent;
mod tinyfish;

pub use aliyun::{AliyunConfig, AliyunEngineType, AliyunProvider};
pub use anysearch::{AnySearchConfig, AnySearchDomain, AnySearchProvider, AnySearchSubDomain};
pub use bocha::{BochaConfig, BochaProvider};
pub use builtin::BuiltinProvider;
pub use credential::{CredentialSource, ProviderAuthentication, ProviderReadiness};
pub use engine::ProviderEngine;
pub use firecrawl::{FirecrawlCategory, FirecrawlConfig, FirecrawlProvider, FirecrawlSource};
pub use http::ProviderHttpConfig;
pub use protocol::{
    ProviderCapabilities, ProviderDescriptor, ProviderReport, ProviderRequest, ProviderResponse,
    ProviderResult, SearchProvider,
};
pub use tavily::{
    TavilyAnswer, TavilyConfig, TavilyCountry, TavilyDate, TavilyProvider, TavilyRawContent,
    TavilySearchDepth, TavilyTopic,
};
pub use tencent::{TencentConfig, TencentIndustry, TencentProvider};
pub use tinyfish::{TinyFishConfig, TinyFishDomainType, TinyFishProvider};

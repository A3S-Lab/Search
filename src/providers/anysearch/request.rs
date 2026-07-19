//! AnySearch MCP request wire types.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::AnySearchDomain;

#[derive(Serialize)]
pub(super) struct AnySearchRpcRequest<'a> {
    pub(super) jsonrpc: &'static str,
    pub(super) id: u64,
    pub(super) method: &'static str,
    pub(super) params: AnySearchCallParams<'a>,
}

#[derive(Serialize)]
pub(super) struct AnySearchCallParams<'a> {
    pub(super) name: &'static str,
    pub(super) arguments: AnySearchArguments<'a>,
}

#[derive(Serialize)]
pub(super) struct AnySearchArguments<'a> {
    pub(super) query: &'a str,
    pub(super) max_results: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) domain: Option<AnySearchDomain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sub_domain: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sub_domain_params: Option<&'a BTreeMap<String, Value>>,
}

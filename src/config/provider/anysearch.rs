//! AnySearch ACL provider parsing.

use a3s_acl::ast::Block;

use crate::providers::{AnySearchConfig, AnySearchDomain, AnySearchProvider, AnySearchSubDomain};
use crate::Result;

use super::common::{
    acl_object_to_json_map, apply_anysearch_http_config, config_error, optional_credential,
    optional_non_empty_string, optional_string, optional_u8, optional_url,
};

pub(super) fn parse(block: &Block, provider: &str) -> Result<AnySearchConfig> {
    let mut config = AnySearchConfig::new()?;

    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(max_results) = optional_u8(block, provider, "max_results")? {
        config = config.with_max_results(max_results)?;
    }
    if let Some(domain) = optional_string(block, provider, "domain")? {
        config = config.with_domain(parse_domain(provider, &domain)?);
    }
    if let Some(sub_domain) = optional_non_empty_string(block, provider, "sub_domain")? {
        config = config.with_sub_domain(AnySearchSubDomain::new(sub_domain)?);
    }
    if let Some(params) = block.attributes.get("sub_domain_params") {
        config = config.with_sub_domain_params(acl_object_to_json_map(
            provider,
            "sub_domain_params",
            params,
        )?);
    }
    config = apply_anysearch_http_config(config, block, provider)?;

    AnySearchProvider::new(config.clone())?;
    Ok(config)
}

fn parse_domain(provider: &str, domain: &str) -> Result<AnySearchDomain> {
    match domain {
        "general" => Ok(AnySearchDomain::General),
        "resource" => Ok(AnySearchDomain::Resource),
        "social_media" => Ok(AnySearchDomain::SocialMedia),
        "finance" => Ok(AnySearchDomain::Finance),
        "academic" => Ok(AnySearchDomain::Academic),
        "legal" => Ok(AnySearchDomain::Legal),
        "health" => Ok(AnySearchDomain::Health),
        "business" => Ok(AnySearchDomain::Business),
        "security" => Ok(AnySearchDomain::Security),
        "ip" => Ok(AnySearchDomain::Ip),
        "code" => Ok(AnySearchDomain::Code),
        "energy" => Ok(AnySearchDomain::Energy),
        "environment" => Ok(AnySearchDomain::Environment),
        "agriculture" => Ok(AnySearchDomain::Agriculture),
        "travel" => Ok(AnySearchDomain::Travel),
        "film" => Ok(AnySearchDomain::Film),
        "gaming" => Ok(AnySearchDomain::Gaming),
        _ => Err(config_error(
            provider,
            "attribute \"domain\" is not a documented AnySearch domain",
        )),
    }
}

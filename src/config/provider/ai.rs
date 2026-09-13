//! ACL parsing for authenticated AI search providers.

use a3s_acl::ast::Block;

use crate::providers::{
    AliyunConfig, AliyunEngineType, BochaConfig, FirecrawlCategory, FirecrawlConfig,
    FirecrawlSource, TencentConfig, TencentIndustry, TinyFishConfig, TinyFishDomainType,
};
use crate::Result;

use super::common::{
    config_error, optional_bool, optional_credential, optional_non_empty_string, optional_string,
    optional_string_list, optional_u8, optional_url, provider_http_config,
};

pub(super) fn parse_tinyfish(block: &Block, provider: &str) -> Result<TinyFishConfig> {
    let mut config = TinyFishConfig::new()?;
    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(purpose) = optional_non_empty_string(block, provider, "purpose")? {
        config = config.with_purpose(purpose)?;
    }
    if let Some(location) = optional_string(block, provider, "location")? {
        config = config.with_location(location)?;
    }
    if let Some(domain_type) = optional_string(block, provider, "domain_type")? {
        config = config.with_domain_type(match domain_type.as_str() {
            "web" => TinyFishDomainType::Web,
            "news" => TinyFishDomainType::News,
            "research_paper" | "research-paper" => TinyFishDomainType::ResearchPaper,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"domain_type\" must be web, news, or research_paper",
                ));
            }
        });
    }
    if let Some(domains) = optional_string_list(block, provider, "include_domains")? {
        config = config.with_include_domains(domains)?;
    }
    if let Some(domains) = optional_string_list(block, provider, "exclude_domains")? {
        config = config.with_exclude_domains(domains)?;
    }
    if let Some(include_thumbnail) = optional_bool(block, provider, "include_thumbnail")? {
        config = config.with_include_thumbnail(include_thumbnail);
    }
    if let Some(http) = provider_http_config(block, provider)? {
        config = config.with_http_config(http);
    }
    Ok(config)
}

pub(super) fn parse_bocha(block: &Block, provider: &str) -> Result<BochaConfig> {
    let mut config = BochaConfig::new()?;
    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(max_results) = optional_u8(block, provider, "max_results")? {
        config = config.with_max_results(max_results)?;
    }
    if let Some(summary) = optional_bool(block, provider, "summary")? {
        config = config.with_summary(summary);
    }
    if let Some(http) = provider_http_config(block, provider)? {
        config = config.with_http_config(http);
    }
    Ok(config)
}

pub(super) fn parse_aliyun(block: &Block, provider: &str) -> Result<AliyunConfig> {
    let mut config = AliyunConfig::new()?;
    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(engine_type) = optional_string(block, provider, "engine_type")? {
        config = config.with_engine_type(match engine_type.as_str() {
            "generic" => AliyunEngineType::Generic,
            "generic_advanced" | "generic-advanced" => AliyunEngineType::GenericAdvanced,
            "lite_advanced" | "lite-advanced" => AliyunEngineType::LiteAdvanced,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"engine_type\" must be generic, generic-advanced, or lite-advanced",
                ));
            }
        })?;
    }
    if let Some(max_results) = optional_u8(block, provider, "max_results")? {
        config = config.with_max_results(max_results)?;
    }
    if let Some(include_main_text) = optional_bool(block, provider, "include_main_text")? {
        config = config.with_include_main_text(include_main_text);
    }
    if let Some(http) = provider_http_config(block, provider)? {
        config = config.with_http_config(http);
    }
    Ok(config)
}

pub(super) fn parse_tencent(block: &Block, provider: &str) -> Result<TencentConfig> {
    let mut config = TencentConfig::new()?;
    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(max_results) = optional_u8(block, provider, "max_results")? {
        config = config.with_max_results(max_results)?;
    }
    if let Some(site) = optional_non_empty_string(block, provider, "site")? {
        config = config.with_site(site)?;
    }
    if let Some(industry) = optional_string(block, provider, "industry")? {
        config = config.with_industry(match industry.as_str() {
            "gov" | "government" => TencentIndustry::Government,
            "news" => TencentIndustry::News,
            "acad" | "academic" => TencentIndustry::Academic,
            "finance" => TencentIndustry::Finance,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"industry\" must be gov, news, acad, or finance",
                ));
            }
        });
    }
    if let Some(http) = provider_http_config(block, provider)? {
        config = config.with_http_config(http);
    }
    Ok(config)
}

pub(super) fn parse_firecrawl(block: &Block, provider: &str) -> Result<FirecrawlConfig> {
    let mut config = FirecrawlConfig::new()?;
    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(max_results) = optional_u8(block, provider, "max_results")? {
        config = config.with_max_results(max_results)?;
    }
    if let Some(location) = optional_non_empty_string(block, provider, "location")? {
        config = config.with_location(location)?;
    }
    if let Some(country) = optional_string(block, provider, "country")? {
        config = config.with_country(country)?;
    }
    if let Some(domains) = optional_string_list(block, provider, "include_domains")? {
        config = config.with_include_domains(domains)?;
    }
    if let Some(domains) = optional_string_list(block, provider, "exclude_domains")? {
        config = config.with_exclude_domains(domains)?;
    }
    if let Some(categories) = optional_string_list(block, provider, "categories")? {
        config = config.with_categories(parse_categories(provider, &categories)?)?;
    }
    if let Some(sources) = optional_string_list(block, provider, "sources")? {
        config = config.with_sources(parse_sources(provider, &sources)?)?;
    }
    if let Some(include_markdown) = optional_bool(block, provider, "include_markdown")? {
        config = config.with_include_markdown(include_markdown);
    }
    if let Some(http) = provider_http_config(block, provider)? {
        config = config.with_http_config(http);
    }
    Ok(config)
}

fn parse_categories(provider: &str, values: &[String]) -> Result<Vec<FirecrawlCategory>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "github" => Ok(FirecrawlCategory::Github),
            "research" => Ok(FirecrawlCategory::Research),
            "pdf" => Ok(FirecrawlCategory::Pdf),
            "developer" => Ok(FirecrawlCategory::Developer),
            _ => Err(config_error(
                provider,
                "attribute \"categories\" entries must be github, research, pdf, or developer",
            )),
        })
        .collect()
}

fn parse_sources(provider: &str, values: &[String]) -> Result<Vec<FirecrawlSource>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "web" => Ok(FirecrawlSource::Web),
            "news" => Ok(FirecrawlSource::News),
            "images" => Ok(FirecrawlSource::Images),
            _ => Err(config_error(
                provider,
                "attribute \"sources\" entries must be web, news, or images",
            )),
        })
        .collect()
}

//! Tavily ACL provider parsing.

use a3s_acl::ast::Block;

use crate::providers::{
    TavilyAnswer, TavilyConfig, TavilyCountry, TavilyDate, TavilyProvider, TavilyRawContent,
    TavilySearchDepth, TavilyTopic,
};
use crate::Result;

use super::common::{
    apply_tavily_http_config, config_error, optional_bool, optional_credential,
    optional_non_empty_string, optional_string, optional_string_list, optional_u8, optional_url,
};

pub(super) fn parse(block: &Block, provider: &str) -> Result<TavilyConfig> {
    let mut config = TavilyConfig::new()?;

    if let Some(endpoint) = optional_url(block, provider, "endpoint")? {
        config = config.with_endpoint(endpoint)?;
    }
    if let Some(api_key) = optional_credential(block, provider, "api_key")? {
        config = config.with_api_key(api_key);
    }
    if let Some(project) = optional_credential(block, provider, "project")? {
        config = config.with_project(project);
    }
    if let Some(depth) = optional_string(block, provider, "search_depth")? {
        config = config.with_search_depth(match depth.as_str() {
            "advanced" => TavilySearchDepth::Advanced,
            "basic" => TavilySearchDepth::Basic,
            "fast" => TavilySearchDepth::Fast,
            "ultra-fast" | "ultra_fast" => TavilySearchDepth::UltraFast,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"search_depth\" must be advanced, basic, fast, or ultra-fast",
                ));
            }
        });
    }
    if let Some(chunks) = optional_u8(block, provider, "chunks_per_source")? {
        config = config.with_chunks_per_source(chunks)?;
    }
    if let Some(max_results) = optional_u8(block, provider, "max_results")? {
        config = config.with_max_results(max_results)?;
    }
    if let Some(topic) = optional_string(block, provider, "topic")? {
        config = config.with_topic(match topic.as_str() {
            "general" => TavilyTopic::General,
            "news" => TavilyTopic::News,
            "finance" => TavilyTopic::Finance,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"topic\" must be general, news, or finance",
                ));
            }
        });
    }
    if let Some(answer) = optional_string(block, provider, "include_answer")? {
        config = config.with_answer(match answer.as_str() {
            "none" => TavilyAnswer::None,
            "basic" => TavilyAnswer::Basic,
            "advanced" => TavilyAnswer::Advanced,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"include_answer\" must be none, basic, or advanced",
                ));
            }
        });
    }
    if let Some(raw_content) = optional_string(block, provider, "include_raw_content")? {
        config = config.with_raw_content(match raw_content.as_str() {
            "none" => TavilyRawContent::None,
            "markdown" => TavilyRawContent::Markdown,
            "text" => TavilyRawContent::Text,
            _ => {
                return Err(config_error(
                    provider,
                    "attribute \"include_raw_content\" must be none, markdown, or text",
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
    if let Some(date) = optional_non_empty_string(block, provider, "start_date")? {
        config = config.with_start_date(TavilyDate::new(date)?);
    }
    if let Some(date) = optional_non_empty_string(block, provider, "end_date")? {
        config = config.with_end_date(TavilyDate::new(date)?);
    }
    if let Some(country) = optional_non_empty_string(block, provider, "country")? {
        config = config.with_country(TavilyCountry::new(country)?);
    }
    if let Some(value) = optional_bool(block, provider, "auto_parameters")? {
        config = config.with_auto_parameters(value);
    }
    if let Some(value) = optional_bool(block, provider, "exact_match")? {
        config = config.with_exact_match(value);
    }
    if let Some(value) = optional_bool(block, provider, "include_usage")? {
        config = config.with_include_usage(value);
    }
    if let Some(value) = optional_bool(block, provider, "include_images")? {
        config = config.with_include_images(value);
    }
    if let Some(value) = optional_bool(block, provider, "include_image_descriptions")? {
        config = config.with_image_descriptions(value);
    }
    if let Some(value) = optional_bool(block, provider, "include_favicon")? {
        config = config.with_favicon(value);
    }
    if let Some(value) = optional_bool(block, provider, "safe_search")? {
        config = config.with_safe_search(value);
    }
    config = apply_tavily_http_config(config, block, provider)?;

    TavilyProvider::new(config.clone())?;
    Ok(config)
}

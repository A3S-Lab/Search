//! Frozen V2 identity for complete caller-visible search results.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{is_canonical_sha256, SearchCascadeReceiptError};
use crate::{
    EngineFailure, EngineOutcome, EngineOutcomeKind, ResultType, SearchImage, SearchReport,
    SearchResult, SearchResults, SearchUsage,
};

const SEARCH_RESULTS_BINDING_V2_DOMAIN: &[u8] = b"a3s/search-results-binding/v2\0";
const MAX_METADATA_DEPTH: usize = 64;

/// A deterministic identity of every caller-visible field in [`SearchResults`].
///
/// Ordered collections retain their order, engine sets and JSON object keys
/// are sorted, strings retain their exact UTF-8 bytes, and finite floating
/// point values use normalized IEEE-754 bits (`-0.0` is encoded as `0.0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchResultsBindingV2 {
    /// Lowercase hexadecimal SHA-256 over the frozen V2 result encoding.
    pub sha256: String,
}

impl SearchResultsBindingV2 {
    /// Binds the complete caller-visible search result container.
    pub fn new(results: &SearchResults) -> Result<Self, SearchCascadeReceiptError> {
        Ok(Self {
            sha256: search_results_sha256(results)?,
        })
    }

    /// Recomputes and validates the result identity.
    pub fn validate(&self, results: &SearchResults) -> Result<(), SearchCascadeReceiptError> {
        if !is_canonical_sha256(&self.sha256) || self.sha256 != search_results_sha256(results)? {
            return Err(SearchCascadeReceiptError::InvalidResultDigest);
        }
        Ok(())
    }
}

fn search_results_sha256(results: &SearchResults) -> Result<String, SearchCascadeReceiptError> {
    let mut encoder = StableEncoder::new();

    encoder.label("results");
    encoder.length(results.items().len());
    for (index, result) in results.items().iter().enumerate() {
        encode_result(&mut encoder, result, index)?;
    }

    encoder.label("suggestions");
    encoder.strings(results.suggestions());
    encoder.label("answers");
    encoder.strings(results.answers());

    encoder.label("images");
    encoder.length(results.images().len());
    for (index, image) in results.images().iter().enumerate() {
        encode_image(&mut encoder, image, &format!("images[{index}]"));
    }

    encoder.label("errors");
    encoder.length(results.errors().len());
    for (engine, message) in results.errors() {
        encoder.string(engine);
        encoder.string(message);
    }

    encoder.label("failures");
    encoder.length(results.failures().len());
    for failure in results.failures() {
        encode_failure(&mut encoder, failure);
    }

    encoder.label("reports");
    encoder.length(results.reports().len());
    for (index, report) in results.reports().iter().enumerate() {
        encode_report(&mut encoder, report, index)?;
    }

    encoder.label("outcomes");
    encoder.length(results.outcomes().len());
    for outcome in results.outcomes() {
        encode_outcome(&mut encoder, outcome);
    }

    encoder.label("count");
    encoder.length(results.count);
    encoder.label("duration_ms");
    encoder.u64(results.duration_ms);

    Ok(encoder.finish())
}

fn encode_result(
    encoder: &mut StableEncoder,
    result: &SearchResult,
    index: usize,
) -> Result<(), SearchCascadeReceiptError> {
    let path = format!("results[{index}]");
    encoder.label("url");
    encoder.string(&result.url);
    encoder.label("title");
    encoder.string(&result.title);
    encoder.label("content");
    encoder.string(&result.content);
    encoder.label("result_type");
    encoder.tag(result_type_tag(result.result_type));

    encoder.label("engines");
    let mut engines = result.engines.iter().collect::<Vec<_>>();
    engines.sort_unstable();
    encoder.length(engines.len());
    for engine in engines {
        encoder.string(engine);
    }

    encoder.label("positions");
    encoder.length(result.positions.len());
    for position in &result.positions {
        encoder.u32(*position);
    }

    encoder.label("score");
    encoder.f64(result.score, &format!("{path}.score"))?;
    encoder.label("relevance_score");
    encoder.optional_f64(result.relevance_score, &format!("{path}.relevance_score"))?;
    encoder.label("thumbnail");
    encoder.optional_string(result.thumbnail.as_deref());
    encoder.label("published_date");
    encoder.optional_string(result.published_date.as_deref());
    encoder.label("favicon");
    encoder.optional_string(result.favicon.as_deref());

    encoder.label("images");
    encoder.length(result.images.len());
    for (image_index, image) in result.images.iter().enumerate() {
        encode_image(encoder, image, &format!("{path}.images[{image_index}]"));
    }

    encoder.label("full_text");
    encoder.optional_string(result.full_text.as_deref());
    Ok(())
}

fn encode_image(encoder: &mut StableEncoder, image: &SearchImage, _path: &str) {
    encoder.label("url");
    encoder.string(&image.url);
    encoder.label("description");
    encoder.optional_string(image.description.as_deref());
}

fn encode_failure(encoder: &mut StableEncoder, failure: &EngineFailure) {
    encoder.label("engine");
    encoder.string(&failure.engine);
    encoder.label("provider");
    encoder.optional_string(failure.provider.as_deref());
    encoder.label("kind");
    encoder.string(&failure.kind);
    encoder.label("message");
    encoder.string(&failure.message);
    encoder.label("transient");
    encoder.boolean(failure.transient);
    encoder.label("retry_after_seconds");
    encoder.optional_u64(failure.retry_after_seconds);
}

fn encode_report(
    encoder: &mut StableEncoder,
    report: &SearchReport,
    index: usize,
) -> Result<(), SearchCascadeReceiptError> {
    encoder.label("engine");
    encoder.string(&report.engine);
    encoder.label("provider");
    encoder.optional_string(report.provider.as_deref());
    encoder.label("request_id");
    encoder.optional_string(report.request_id.as_deref());
    encoder.label("total_results");
    encoder.optional_u64(report.total_results);
    encoder.label("response_time_ms");
    encoder.optional_u64(report.response_time_ms);
    encoder.label("usage");
    match &report.usage {
        Some(usage) => {
            encoder.present();
            encode_usage(encoder, usage, index)?;
        }
        None => encoder.absent(),
    }
    encoder.label("metadata");
    encoder.length(report.metadata.len());
    for (key, value) in &report.metadata {
        encoder.string(key);
        encode_json_value(
            encoder,
            value,
            0,
            &format!("reports[{index}].metadata.{key}"),
        )?;
    }
    Ok(())
}

fn encode_usage(
    encoder: &mut StableEncoder,
    usage: &SearchUsage,
    report_index: usize,
) -> Result<(), SearchCascadeReceiptError> {
    encoder.label("credits");
    encoder.optional_f64(
        usage.credits,
        &format!("reports[{report_index}].usage.credits"),
    )
}

fn encode_outcome(encoder: &mut StableEncoder, outcome: &EngineOutcome) {
    encoder.label("engine");
    encoder.string(&outcome.engine);
    encoder.label("shortcut");
    encoder.string(&outcome.shortcut);
    encoder.label("provider");
    encoder.optional_string(outcome.provider.as_deref());
    encoder.label("kind");
    encoder.tag(outcome_kind_tag(outcome.kind));
    encoder.label("result_count");
    encoder.length(outcome.result_count);
    encoder.label("duration_ms");
    encoder.u64(outcome.duration_ms);
    encoder.label("failure");
    match &outcome.failure {
        Some(failure) => {
            encoder.present();
            encode_failure(encoder, failure);
        }
        None => encoder.absent(),
    }
}

fn encode_json_value(
    encoder: &mut StableEncoder,
    value: &serde_json::Value,
    depth: usize,
    path: &str,
) -> Result<(), SearchCascadeReceiptError> {
    if depth > MAX_METADATA_DEPTH {
        return Err(SearchCascadeReceiptError::InvalidResultValue {
            field: path.to_string(),
        });
    }
    match value {
        serde_json::Value::Null => encoder.tag(0),
        serde_json::Value::Bool(value) => {
            encoder.tag(1);
            encoder.boolean(*value);
        }
        serde_json::Value::Number(value) => {
            encoder.tag(2);
            encoder.string(&value.to_string());
        }
        serde_json::Value::String(value) => {
            encoder.tag(3);
            encoder.string(value);
        }
        serde_json::Value::Array(values) => {
            encoder.tag(4);
            encoder.length(values.len());
            for (index, value) in values.iter().enumerate() {
                encode_json_value(encoder, value, depth + 1, &format!("{path}[{index}]"))?;
            }
        }
        serde_json::Value::Object(values) => {
            encoder.tag(5);
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
            encoder.length(entries.len());
            for (key, value) in entries {
                encoder.string(key);
                encode_json_value(encoder, value, depth + 1, &format!("{path}.{key}"))?;
            }
        }
    }
    Ok(())
}

fn result_type_tag(value: ResultType) -> u8 {
    match value {
        ResultType::Web => 0,
        ResultType::Image => 1,
        ResultType::Video => 2,
        ResultType::News => 3,
        ResultType::Map => 4,
        ResultType::File => 5,
        ResultType::Answer => 6,
        ResultType::Infobox => 7,
        ResultType::Suggestion => 8,
    }
}

fn outcome_kind_tag(value: EngineOutcomeKind) -> u8 {
    match value {
        EngineOutcomeKind::Success => 0,
        EngineOutcomeKind::Empty => 1,
        EngineOutcomeKind::Failure => 2,
        EngineOutcomeKind::Timeout => 3,
        EngineOutcomeKind::Rejected => 4,
        EngineOutcomeKind::CircuitOpen => 5,
    }
}

struct StableEncoder {
    hasher: Sha256,
}

impl StableEncoder {
    fn new() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(SEARCH_RESULTS_BINDING_V2_DOMAIN);
        Self { hasher }
    }

    fn finish(self) -> String {
        format!("{:x}", self.hasher.finalize())
    }

    fn label(&mut self, value: &str) {
        self.string(value);
    }

    fn string(&mut self, value: &str) {
        self.length(value.len());
        self.hasher.update(value.as_bytes());
    }

    fn strings(&mut self, values: &[String]) {
        self.length(values.len());
        for value in values {
            self.string(value);
        }
    }

    fn length(&mut self, value: usize) {
        self.hasher.update((value as u128).to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.hasher.update(value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.hasher.update(value.to_be_bytes());
    }

    fn boolean(&mut self, value: bool) {
        self.tag(u8::from(value));
    }

    fn tag(&mut self, value: u8) {
        self.hasher.update([value]);
    }

    fn present(&mut self) {
        self.tag(1);
    }

    fn absent(&mut self) {
        self.tag(0);
    }

    fn optional_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.present();
                self.string(value);
            }
            None => self.absent(),
        }
    }

    fn optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.present();
                self.u64(value);
            }
            None => self.absent(),
        }
    }

    fn optional_f64(
        &mut self,
        value: Option<f64>,
        path: &str,
    ) -> Result<(), SearchCascadeReceiptError> {
        match value {
            Some(value) => {
                self.present();
                self.f64(value, path)
            }
            None => {
                self.absent();
                Ok(())
            }
        }
    }

    fn f64(&mut self, value: f64, path: &str) -> Result<(), SearchCascadeReceiptError> {
        if !value.is_finite() {
            return Err(SearchCascadeReceiptError::InvalidResultValue {
                field: path.to_string(),
            });
        }
        let normalized = if value == 0.0 { 0.0 } else { value };
        self.hasher.update(normalized.to_bits().to_be_bytes());
        Ok(())
    }
}

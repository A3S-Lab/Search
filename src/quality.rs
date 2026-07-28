//! Domain-agnostic result quality and tier-cascade decisions.

use std::collections::HashSet;
use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::{SearchQuery, SearchResult, SearchResults};

/// Observable, topic-neutral quality signals for one combined result set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchQuality {
    /// Results with an absolute HTTP(S) URL.
    pub usable_result_count: usize,
    /// Distinct normalized hosts represented by usable results.
    pub unique_host_count: usize,
    /// Distinct engines that contributed at least one usable result.
    pub contributing_engine_count: usize,
    /// Results independently returned by at least two engines.
    pub consensus_result_count: usize,
    /// Results meeting the requested alignment threshold.
    pub aligned_result_count: usize,
    /// Mean language-neutral query alignment across usable results.
    pub mean_query_match: f64,
}

impl SearchQuality {
    /// Evaluates a result set against a query without publisher, topic, or
    /// language-specific rules.
    pub fn evaluate(query: &str, results: &SearchResults, alignment_threshold: f64) -> Self {
        let alignment_threshold = normalized_threshold(alignment_threshold);
        let mut hosts = HashSet::new();
        let mut engines = HashSet::new();
        let mut usable_result_count = 0usize;
        let mut consensus_result_count = 0usize;
        let mut aligned_result_count = 0usize;
        let mut alignment_total = 0.0;

        for result in results.items() {
            let Ok(url) = url::Url::parse(result.url.trim()) else {
                continue;
            };
            if !matches!(url.scheme(), "http" | "https") {
                continue;
            }
            let Some(host) = url.host_str() else {
                continue;
            };

            usable_result_count = usable_result_count.saturating_add(1);
            hosts.insert(host.trim_start_matches("www.").to_ascii_lowercase());
            engines.extend(result.engines.iter().cloned());
            if result.engines.len() >= 2 {
                consensus_result_count = consensus_result_count.saturating_add(1);
            }
            let alignment = result
                .query_match_score
                .and_then(normalized_alignment)
                .unwrap_or_else(|| query_match_score(query, result));
            alignment_total += alignment;
            if alignment >= alignment_threshold {
                aligned_result_count = aligned_result_count.saturating_add(1);
            }
        }

        Self {
            usable_result_count,
            unique_host_count: hosts.len(),
            contributing_engine_count: engines.len(),
            consensus_result_count,
            aligned_result_count,
            mean_query_match: if usable_result_count == 0 {
                0.0
            } else {
                alignment_total / usable_result_count as f64
            },
        }
    }
}

/// Caller-selected floor that decides whether another search tier is needed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchQualityFloor {
    /// Minimum usable HTTP(S) results.
    pub min_usable_results: usize,
    /// Minimum distinct normalized hosts.
    pub min_unique_hosts: usize,
    /// Minimum distinct contributing engines.
    pub min_contributing_engines: usize,
    /// Minimum aligned results.
    pub min_aligned_results: usize,
    /// Minimum results independently returned by at least two engines.
    pub min_consensus_results: usize,
    /// Per-result query-alignment threshold.
    pub min_query_match: f64,
    /// Minimum mean query alignment across all usable results.
    pub min_mean_query_match: f64,
}

impl SearchQualityFloor {
    /// Creates a conservative generic floor for a requested display limit.
    pub fn for_limit(limit: usize) -> Self {
        let target = limit.min(5);
        Self {
            min_usable_results: target,
            min_unique_hosts: target.min(3),
            min_contributing_engines: usize::from(target > 0),
            min_aligned_results: target.div_ceil(2),
            min_consensus_results: 0,
            min_query_match: 0.35,
            min_mean_query_match: 0.30,
        }
    }

    /// Returns whether the observed quality satisfies this floor.
    pub fn is_met(&self, quality: &SearchQuality) -> bool {
        quality.usable_result_count >= self.min_usable_results
            && quality.unique_host_count >= self.min_unique_hosts
            && quality.contributing_engine_count >= self.min_contributing_engines
            && quality.aligned_result_count >= self.min_aligned_results
            && quality.consensus_result_count >= self.min_consensus_results
            && quality.mean_query_match >= normalized_threshold(self.min_mean_query_match)
    }
}

impl Default for SearchQualityFloor {
    fn default() -> Self {
        Self::for_limit(10)
    }
}

/// Decision after one tier has been merged into a search cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchTierDecision {
    /// The combined result set satisfies the configured quality floor.
    Stop,
    /// A lower tier should run if one is available.
    Continue,
}

/// Audit record for one executed search tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchTierReport {
    /// Caller-defined tier identifier.
    pub tier: String,
    /// Quality after this tier was merged.
    pub combined_quality: SearchQuality,
    /// Cascade decision after this tier.
    pub decision: SearchTierDecision,
}

/// Stateful merger and quality gate for lazily executed search tiers.
#[derive(Debug)]
pub struct SearchCascade {
    query: SearchQuery,
    floor: SearchQualityFloor,
    results: SearchResults,
    reports: Vec<SearchTierReport>,
}

impl SearchCascade {
    /// Starts a cascade for one query and quality floor.
    pub fn new(query: SearchQuery, floor: SearchQualityFloor) -> Self {
        Self {
            query,
            floor,
            results: SearchResults::new(),
            reports: Vec::new(),
        }
    }

    /// Merges one lazily executed tier and returns whether another is needed.
    pub fn push_tier(
        &mut self,
        tier: impl Into<String>,
        results: SearchResults,
    ) -> SearchTierDecision {
        self.results.merge(results);
        let quality = self.quality();
        let decision = if self.floor.is_met(&quality) {
            SearchTierDecision::Stop
        } else {
            SearchTierDecision::Continue
        };
        self.reports.push(SearchTierReport {
            tier: tier.into(),
            combined_quality: quality,
            decision,
        });
        decision
    }

    /// Executes and merges one tier only while the quality floor is unmet.
    ///
    /// The closure is not invoked after an earlier tier has satisfied the
    /// floor, so callers can keep expensive transports such as headless
    /// browsers uninitialized on the healthy fast path.
    pub async fn run_tier_if_needed<F, Fut>(
        &mut self,
        tier: impl Into<String>,
        run: F,
    ) -> Option<SearchTierDecision>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = SearchResults>,
    {
        if !self.needs_next_tier() {
            return None;
        }
        Some(self.push_tier(tier, run().await))
    }

    /// Returns the quality of all tiers merged so far.
    pub fn quality(&self) -> SearchQuality {
        SearchQuality::evaluate(&self.query.query, &self.results, self.floor.min_query_match)
    }

    /// Returns whether a lower tier is still required.
    pub fn needs_next_tier(&self) -> bool {
        !self.floor.is_met(&self.quality())
    }

    /// Returns the current combined results.
    pub fn results(&self) -> &SearchResults {
        &self.results
    }

    /// Returns the executed-tier audit trail.
    pub fn reports(&self) -> &[SearchTierReport] {
        &self.reports
    }

    /// Consumes the cascade and returns its combined results.
    pub fn into_results(self) -> SearchResults {
        self.results
    }
}

/// Measures how much of a query is represented by a result without topic,
/// publisher, or language-specific rules.
///
/// Multi-term queries use length-weighted term coverage so one generic word
/// cannot satisfy a longer request. Queries that do not expose multiple word
/// boundaries use normalized Unicode character n-grams. In both cases the
/// title and URL carry more weight than the snippet, where incidental matches
/// and page boilerplate are more common.
pub fn query_match_score(query: &str, result: &SearchResult) -> f64 {
    const TITLE_URL_WEIGHT: f64 = 0.75;
    const SNIPPET_WEIGHT: f64 = 1.0 - TITLE_URL_WEIGHT;

    let query_units = normalized_query_units(query);
    let query_characters = normalized_characters(query);
    if query_characters.is_empty() {
        return 0.0;
    }

    let field_score = |visible: &str| {
        if query_units.len() > 1 {
            query_unit_coverage(&query_units, visible)
        } else {
            character_gram_coverage(&query_characters, visible)
        }
    };
    let headline_score = field_score(&format!("{} {}", result.title, result.url));
    let snippet_score = field_score(&result.content);
    TITLE_URL_WEIGHT.mul_add(headline_score, SNIPPET_WEIGHT * snippet_score)
}

fn normalized_query_units(value: &str) -> Vec<Vec<char>> {
    let mut seen = HashSet::new();
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter_map(|unit| {
            let normalized = normalized_characters(unit);
            (!normalized.is_empty() && seen.insert(normalized.clone())).then_some(normalized)
        })
        .collect()
}

fn query_unit_coverage(query_units: &[Vec<char>], visible: &str) -> f64 {
    let visible = normalized_characters(visible);
    let total_weight = query_units.iter().map(Vec::len).sum::<usize>();
    if visible.is_empty() || total_weight == 0 {
        return 0.0;
    }
    let matched_weight = query_units
        .iter()
        .filter(|unit| contains_characters(&visible, unit))
        .map(Vec::len)
        .sum::<usize>();
    matched_weight as f64 / total_weight as f64
}

fn character_gram_coverage(query: &[char], visible: &str) -> f64 {
    let visible = normalized_characters(visible);
    if visible.is_empty() {
        return 0.0;
    }
    let gram_size = query.len().min(3);
    let query_grams = character_grams(query, gram_size);
    let visible_grams = character_grams(&visible, gram_size);
    if query_grams.is_empty() {
        return 0.0;
    }
    let matched = query_grams
        .iter()
        .filter(|gram| visible_grams.contains(*gram))
        .count();
    matched as f64 / query_grams.len() as f64
}

fn contains_characters(haystack: &[char], needle: &[char]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn normalized_characters(value: &str) -> Vec<char> {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn character_grams(value: &[char], size: usize) -> HashSet<Vec<char>> {
    if size == 0 || value.len() < size {
        return HashSet::new();
    }
    value.windows(size).map(<[char]>::to_vec).collect()
}

fn normalized_threshold(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn normalized_alignment(value: f64) -> Option<f64> {
    value.is_finite().then(|| value.clamp(0.0, 1.0))
}

#[cfg(test)]
#[path = "quality/tests.rs"]
mod tests;

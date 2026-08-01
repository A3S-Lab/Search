//! Domain-agnostic result quality and tier-cascade decisions.

use std::collections::{HashMap, HashSet};
use std::future::Future;

use serde::{Deserialize, Serialize};
use unicode_script::{Script, UnicodeScript};

use crate::evidence::{
    character_grams, contains_characters, normalized_characters, normalized_full_text_evidence,
    normalized_query_units, normalized_result_evidence_fields,
    query_unit_is_represented_with_exactness, query_units_with_source, QueryEvidence,
};
use crate::{SearchQuery, SearchResult, SearchResults};

const TITLE_WEIGHT: f64 = 0.50;
const SNIPPET_WEIGHT: f64 = 0.45;
const URL_WEIGHT: f64 = 1.0 - TITLE_WEIGHT - SNIPPET_WEIGHT;

mod receipt;

pub use receipt::{
    SearchCascadeCounts, SearchCascadeOutcomeV1, SearchCascadeReceiptBindingV1,
    SearchCascadeReceiptError, SearchCascadeReceiptV1, SearchQueryBindingV1,
    SearchResultsBindingV1, SEARCH_CASCADE_RECEIPT_V1_SCHEMA,
};

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
    /// Results meeting the requested local-or-marginal alignment threshold
    /// when the ranked evidence window also covers the query as a set.
    pub aligned_result_count: usize,
    /// Mean language-neutral query alignment across usable results.
    pub mean_query_match: f64,
}

/// A deterministic evidence-gap query for one later retrieval tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchQueryRefinement {
    /// Query sent to the later retrieval tier.
    pub query: String,
    /// Unique normalized units in the original query.
    pub total_units: usize,
    /// Original units represented in the inspected evidence window.
    pub covered_units: usize,
    /// Original units retained in the refined query, including bounded context.
    pub retained_units: usize,
}

impl SearchQuality {
    /// Evaluates a result set against a query without publisher, topic, or
    /// language-specific rules.
    pub fn evaluate(query: &str, results: &SearchResults, alignment_threshold: f64) -> Self {
        Self::evaluate_results(query, results.items().iter(), alignment_threshold)
    }

    fn evaluate_ranked_head(
        query: &str,
        results: &SearchResults,
        alignment_threshold: f64,
        result_limit: usize,
    ) -> Self {
        Self::evaluate_results(
            query,
            results.items().iter().take(result_limit),
            alignment_threshold,
        )
    }

    fn evaluate_results<'a>(
        query: &str,
        results: impl IntoIterator<Item = &'a SearchResult>,
        alignment_threshold: f64,
    ) -> Self {
        let alignment_threshold = normalized_threshold(alignment_threshold);
        let usable_results = results
            .into_iter()
            .filter_map(|result| {
                let url = url::Url::parse(result.url.trim()).ok()?;
                if !matches!(url.scheme(), "http" | "https") {
                    return None;
                }
                let host = url.host_str()?;
                Some((result, host.trim_start_matches("www.").to_ascii_lowercase()))
            })
            .collect::<Vec<_>>();
        let local_alignments = usable_results
            .iter()
            .map(|(result, _)| result_quality_alignment(query, result))
            .collect::<Vec<_>>();
        let locally_aligned_results = usable_results
            .iter()
            .zip(&local_alignments)
            .filter_map(|((result, _), alignment)| {
                (*alignment >= alignment_threshold).then_some(*result)
            })
            .collect::<Vec<_>>();
        let (evidence_coverage, every_unit_represented) =
            query_evidence_coverage(query, &locally_aligned_results);
        // Complementary rows may complete a composite request only when each
        // contributing row independently reaches the unchanged local
        // alignment floor. This prevents unrelated weak pages from stitching
        // isolated query fragments into an apparent set-level pass.
        let evidence_breadth_met = every_unit_represented
            && evidence_coverage >= (2.0 * alignment_threshold).clamp(0.0, 1.0);
        let mut hosts = HashSet::new();
        let mut engines = HashSet::new();
        let mut consensus_result_count = 0usize;
        let mut aligned_result_count = 0usize;
        let mut alignment_total = 0.0;
        let query_evidence = QueryEvidence::new(query);
        let mut covered_evidence = HashSet::new();

        for ((result, host), alignment) in usable_results.iter().zip(local_alignments) {
            hosts.insert(host.clone());
            engines.extend(result.engines.iter().cloned());
            if result.engines.len() >= 2 {
                consensus_result_count = consensus_result_count.saturating_add(1);
            }
            let matching_evidence = query_evidence.matching_atoms(result);
            let marginal_coverage =
                query_evidence.marginal_coverage(&matching_evidence, &covered_evidence);
            let set_aware_alignment = alignment + marginal_coverage - alignment * marginal_coverage;
            alignment_total += alignment;
            if evidence_breadth_met && set_aware_alignment >= alignment_threshold {
                aligned_result_count = aligned_result_count.saturating_add(1);
                covered_evidence.extend(matching_evidence);
            }
        }

        let usable_result_count = usable_results.len();
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

fn result_quality_alignment(query: &str, result: &SearchResult) -> f64 {
    let base_alignment = result
        .query_match_score
        .and_then(normalized_alignment)
        .unwrap_or_else(|| query_match_score(query, result));
    full_text_query_match_score(query, result).map_or(base_alignment, |full_text_alignment| {
        base_alignment.max(full_text_alignment)
    })
}

/// Derives one shorter query from the least-represented evidence in the current
/// ranked head.
pub fn refine_query_for_evidence(
    query: &str,
    results: &SearchResults,
    result_limit: usize,
) -> Option<SearchQueryRefinement> {
    refine_query_portfolio(query, results, result_limit, 1)
        .into_iter()
        .next()
}

/// Derives a bounded portfolio for later engines without increasing calls.
///
/// Every query targets a disjoint subset of the least-supported normalized
/// units and retains deterministic context covering at least half of the
/// original normalized character weight. That keeps refined retrieval capable
/// of meeting the unchanged original-query alignment floor. Callers can assign
/// at most one portfolio entry to each engine already planned for the next
/// tier. The mechanism contains no language, topic, source, or provider rules.
pub fn refine_query_portfolio(
    query: &str,
    results: &SearchResults,
    result_limit: usize,
    maximum_queries: usize,
) -> Vec<SearchQueryRefinement> {
    let units = query_units_with_source(query);
    if units.len() < 3 || result_limit == 0 || maximum_queries == 0 {
        return Vec::new();
    }

    let visible = results
        .items()
        .iter()
        .take(result_limit)
        .map(normalized_result_evidence_fields)
        .collect::<Vec<_>>();
    let support = units
        .iter()
        .map(|unit| {
            visible
                .iter()
                .filter(|fields| {
                    query_unit_is_represented_with_exactness(
                        &unit.normalized,
                        fields,
                        unit.requires_exact,
                    )
                })
                .count()
        })
        .collect::<Vec<_>>();
    let covered_units = support.iter().filter(|count| **count > 0).count();
    let Some(minimum_support) = support.iter().copied().min() else {
        return Vec::new();
    };
    if !visible.is_empty() && minimum_support == visible.len() {
        return Vec::new();
    }

    let targets = support
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == minimum_support).then_some(index))
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Vec::new();
    }

    let mut context_order = (0..units.len()).collect::<Vec<_>>();
    context_order.sort_by(|left, right| {
        support[*right]
            .cmp(&support[*left])
            .then_with(|| {
                units[*right]
                    .normalized
                    .len()
                    .cmp(&units[*left].normalized.len())
            })
            .then_with(|| left.cmp(right))
    });
    let minimum_retained_weight = units
        .iter()
        .map(|unit| unit.normalized.len())
        .sum::<usize>()
        .div_ceil(2);

    let query_count = maximum_queries.min(targets.len());
    let mut groups = vec![Vec::new(); query_count];
    for (position, target) in targets.into_iter().enumerate() {
        groups[position % query_count].push(target);
    }

    groups
        .into_iter()
        .filter_map(|mut retained| {
            let mut retained_weight = retained
                .iter()
                .map(|index| units[*index].normalized.len())
                .sum::<usize>();
            let mut added_context = false;
            for context in &context_order {
                if retained.contains(context) {
                    continue;
                }
                retained.push(*context);
                retained_weight = retained_weight.saturating_add(units[*context].normalized.len());
                added_context = true;
                if retained_weight >= minimum_retained_weight {
                    break;
                }
            }
            retained.sort_unstable();
            retained.dedup();
            if !added_context || retained.len() == units.len() {
                return None;
            }
            Some(SearchQueryRefinement {
                query: retained
                    .iter()
                    .map(|index| units[*index].source.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                total_units: units.len(),
                covered_units,
                retained_units: retained.len(),
            })
        })
        .collect()
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

    /// Evaluates the smallest ranked result prefix that could satisfy this
    /// floor. Provider tail rows outside that caller-visible evidence window
    /// cannot force an otherwise sufficient cascade to run another tier, and
    /// stronger rows below a weak head cannot be cherry-picked into a pass.
    pub fn evaluate(&self, query: &str, results: &SearchResults) -> SearchQuality {
        let result_limit = self
            .min_usable_results
            .max(self.min_unique_hosts)
            .max(self.min_contributing_engines)
            .max(self.min_aligned_results)
            .max(self.min_consensus_results);
        SearchQuality::evaluate_ranked_head(query, results, self.min_query_match, result_limit)
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
        crate::aggregator::rerank_for_query(&self.query.query, self.results.items_mut());
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
        self.floor.evaluate(&self.query.query, &self.results)
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
/// Multi-term queries combine length-weighted exact-unit coverage with
/// normalized Unicode character n-grams. Unsegmented queries can collect
/// adjacent-character evidence across the visible title and snippet, while
/// mixed-script queries must retain matching evidence from each substantive
/// query script. One generic word or a result written in only one part of a
/// mixed-script request cannot satisfy a longer request. The URL remains a
/// weak auxiliary signal, so a query-shaped path cannot substitute for
/// provider-visible result text.
pub fn query_match_score(query: &str, result: &SearchResult) -> f64 {
    let query_units = normalized_query_units(query);
    let query_characters = query_units
        .iter()
        .flat_map(|unit| unit.iter().copied())
        .collect::<Vec<_>>();
    if query_characters.is_empty() {
        return 0.0;
    }

    let unsegmented_query = query_units.len() == 1;
    let query_scripts = query_script_characters(&query_characters);
    let field_score = |visible: &str| {
        let visible = normalized_characters(visible);
        query_field_match_score(
            &query_units,
            &query_characters,
            &query_scripts,
            &visible,
            unsegmented_query,
        )
    };
    let title_score = field_score(&result.title);
    let snippet_score = field_score(&result.content);
    let url_score = field_score(&result.url);
    let field_weighted_score = TITLE_WEIGHT.mul_add(
        title_score,
        SNIPPET_WEIGHT.mul_add(snippet_score, URL_WEIGHT * url_score),
    );
    let combined_visible_score = field_score(&format!("{} {}", result.title, result.content));
    let combined_visible_weight = if unsegmented_query {
        1.0
    } else {
        TITLE_WEIGHT.max(SNIPPET_WEIGHT)
    };
    field_weighted_score.max(combined_visible_score * combined_visible_weight)
}

fn full_text_query_match_score(query: &str, result: &SearchResult) -> Option<f64> {
    let visible = normalized_full_text_evidence(result)?;
    let query_units = normalized_query_units(query);
    let query_characters = query_units
        .iter()
        .flat_map(|unit| unit.iter().copied())
        .collect::<Vec<_>>();
    if query_characters.is_empty() {
        return Some(0.0);
    }
    let query_scripts = query_script_characters(&query_characters);
    Some(
        query_field_match_score(
            &query_units,
            &query_characters,
            &query_scripts,
            &visible,
            query_units.len() == 1,
        ) * SNIPPET_WEIGHT,
    )
}

fn query_field_match_score(
    query_units: &[Vec<char>],
    query_characters: &[char],
    query_scripts: &HashMap<Script, HashSet<char>>,
    visible: &[char],
    unsegmented_query: bool,
) -> f64 {
    let script_overlap = query_script_overlap(query_scripts, visible);
    let character_score =
        character_gram_coverage(query_characters, visible, unsegmented_query) * script_overlap;
    let lexical_score = if query_units.len() > 1 {
        query_unit_coverage(query_units, visible) * script_overlap
    } else {
        0.0
    };
    lexical_score.max(character_score)
}

fn query_evidence_coverage(query: &str, results: &[&SearchResult]) -> (f64, bool) {
    let query_units = query_units_with_source(query);
    let normalized_units = query_units
        .iter()
        .map(|unit| unit.normalized.clone())
        .collect::<Vec<_>>();
    let query_characters = query_units
        .iter()
        .flat_map(|unit| unit.normalized.iter().copied())
        .collect::<Vec<_>>();
    if query_characters.is_empty() || results.is_empty() {
        return (0.0, false);
    }

    let visible = results
        .iter()
        .flat_map(|result| normalized_result_evidence_fields(result))
        .collect::<Vec<_>>();
    let visible_characters = visible
        .iter()
        .flat_map(|field| field.iter().copied())
        .collect::<Vec<_>>();
    let query_scripts = query_script_characters(&query_characters);
    let script_overlap = query_script_overlap(&query_scripts, &visible_characters);
    let lexical_coverage = if query_units.len() > 1 {
        query_unit_coverage_across_fields(&normalized_units, &visible)
    } else {
        0.0
    };
    let character_coverage =
        character_gram_coverage_across_fields(&query_characters, &visible, query_units.len() == 1);
    let every_unit_represented = match query_units.as_slice() {
        [unit] if !unit.requires_exact => true,
        _ => query_units.iter().all(|unit| {
            query_unit_is_represented_with_exactness(
                &unit.normalized,
                &visible,
                unit.requires_exact,
            )
        }),
    };
    (
        lexical_coverage.max(character_coverage) * script_overlap,
        every_unit_represented,
    )
}

fn query_unit_coverage(query_units: &[Vec<char>], visible: &[char]) -> f64 {
    let total_weight = query_units.iter().map(Vec::len).sum::<usize>();
    if visible.is_empty() || total_weight == 0 {
        return 0.0;
    }
    let matched_weight = query_units
        .iter()
        .filter(|unit| contains_characters(visible, unit))
        .map(Vec::len)
        .sum::<usize>();
    matched_weight as f64 / total_weight as f64
}

fn query_unit_coverage_across_fields(query_units: &[Vec<char>], visible: &[Vec<char>]) -> f64 {
    let total_weight = query_units.iter().map(Vec::len).sum::<usize>();
    if visible.is_empty() || total_weight == 0 {
        return 0.0;
    }
    let matched_weight = query_units
        .iter()
        .filter(|unit| visible.iter().any(|field| contains_characters(field, unit)))
        .map(Vec::len)
        .sum::<usize>();
    matched_weight as f64 / total_weight as f64
}

fn character_gram_coverage(query: &[char], visible: &[char], use_shorter_grams: bool) -> f64 {
    if visible.is_empty() {
        return 0.0;
    }
    let longest_gram = query.len().min(3);
    let longest_coverage = gram_coverage(query, visible, longest_gram);
    if longest_gram < 3 || !use_shorter_grams {
        return longest_coverage;
    }

    let adjacent_coverage = gram_coverage(query, visible, 2);
    let character_coverage = gram_coverage(query, visible, 1);
    longest_coverage.max((2.0 * adjacent_coverage + character_coverage) / 3.0)
}

fn character_gram_coverage_across_fields(
    query: &[char],
    visible: &[Vec<char>],
    use_shorter_grams: bool,
) -> f64 {
    if visible.is_empty() {
        return 0.0;
    }
    let longest_gram = query.len().min(3);
    let longest_coverage = gram_coverage_across_fields(query, visible, longest_gram);
    if longest_gram < 3 || !use_shorter_grams {
        return longest_coverage;
    }

    let adjacent_coverage = gram_coverage_across_fields(query, visible, 2);
    let character_coverage = gram_coverage_across_fields(query, visible, 1);
    longest_coverage.max((2.0 * adjacent_coverage + character_coverage) / 3.0)
}

fn gram_coverage(query: &[char], visible: &[char], gram_size: usize) -> f64 {
    let query_grams = character_grams(query, gram_size);
    let visible_grams = character_grams(visible, gram_size);
    if query_grams.is_empty() {
        return 0.0;
    }
    let matched = query_grams
        .iter()
        .filter(|gram| visible_grams.contains(*gram))
        .count();
    matched as f64 / query_grams.len() as f64
}

fn gram_coverage_across_fields(query: &[char], visible: &[Vec<char>], gram_size: usize) -> f64 {
    let query_grams = character_grams(query, gram_size);
    if query_grams.is_empty() {
        return 0.0;
    }
    let visible_grams = visible
        .iter()
        .flat_map(|field| character_grams(field, gram_size))
        .collect::<HashSet<_>>();
    let matched = query_grams
        .iter()
        .filter(|gram| visible_grams.contains(*gram))
        .count();
    matched as f64 / query_grams.len() as f64
}

fn query_script_characters(query: &[char]) -> HashMap<Script, HashSet<char>> {
    let mut query_scripts = HashMap::<Script, HashSet<char>>::new();
    for character in query {
        let script = character.script();
        if !matches!(script, Script::Common | Script::Inherited | Script::Unknown) {
            query_scripts.entry(script).or_default().insert(*character);
        }
    }
    query_scripts
}

fn query_script_overlap(query_scripts: &HashMap<Script, HashSet<char>>, visible: &[char]) -> f64 {
    if query_scripts.len() <= 1 {
        return 1.0;
    }

    let visible = visible.iter().copied().collect::<HashSet<_>>();
    let matched_scripts = query_scripts
        .values()
        .filter(|characters| {
            characters
                .iter()
                .any(|character| visible.contains(character))
        })
        .count();
    let coverage = matched_scripts as f64 / query_scripts.len() as f64;
    coverage * coverage
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

#[cfg(test)]
#[path = "quality/receipt_tests.rs"]
mod receipt_tests;

#[cfg(test)]
#[path = "quality/receipt_binding_tests.rs"]
mod receipt_binding_tests;

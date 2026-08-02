//! Domain-neutral rank-fusion policy and provider-local score calibration.

use serde::{Deserialize, Serialize};

use crate::SearchResult;

const DEFAULT_RRF_RANK_CONSTANT: f64 = 60.0;
const DEFAULT_NATIVE_RELEVANCE_WEIGHT: f64 = 0.2;
const MAX_RRF_RANK_CONSTANT: f64 = 1_000_000.0;

/// Generic rank-fusion policy used by [`crate::Aggregator`].
///
/// The policy contains no query, topic, host, publisher, entity, or language
/// rules. Native relevance is calibrated within each provider response before
/// it contributes, so incomparable provider score scales are never multiplied
/// directly across engines.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RankingConfig {
    /// Constant in weighted reciprocal-rank fusion.
    ///
    /// Larger values make evidence repeated across engines more important
    /// relative to small position differences inside one engine.
    pub rrf_rank_constant: f64,
    /// Strength of provider-local native relevance in `0.0..=1.0`.
    pub native_relevance_weight: f64,
}

impl RankingConfig {
    pub(crate) fn sanitized(self) -> Self {
        let defaults = Self::default();
        Self {
            rrf_rank_constant: if self.rrf_rank_constant.is_finite()
                && (0.0..=MAX_RRF_RANK_CONSTANT).contains(&self.rrf_rank_constant)
            {
                self.rrf_rank_constant
            } else {
                defaults.rrf_rank_constant
            },
            native_relevance_weight: normalized_weight(
                self.native_relevance_weight,
                defaults.native_relevance_weight,
            ),
        }
    }

    pub(crate) fn reciprocal_rank_score(self, position: u32) -> f64 {
        let position = position.max(1);
        (self.rrf_rank_constant + 1.0) / (self.rrf_rank_constant + f64::from(position))
    }

    pub(crate) fn native_relevance_factor(self, percentile: f64) -> f64 {
        let percentile = normalized_unit_interval(percentile, 0.5);
        1.0 + self.native_relevance_weight * (percentile - 0.5)
    }

    pub(crate) fn is_valid(self) -> bool {
        self == self.sanitized()
    }
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            rrf_rank_constant: DEFAULT_RRF_RANK_CONSTANT,
            native_relevance_weight: DEFAULT_NATIVE_RELEVANCE_WEIGHT,
        }
    }
}

pub(crate) fn calibrated_native_relevance(results: &[SearchResult]) -> Vec<f64> {
    let mut scale = results
        .iter()
        .filter_map(|result| result.relevance_score)
        .filter(|score| score.is_finite())
        .map(|score| score.clamp(0.0, 1.0))
        .collect::<Vec<_>>();
    scale.sort_unstable_by(f64::total_cmp);
    scale.dedup_by(|left, right| left.total_cmp(right).is_eq());

    if scale.len() < 2 {
        return vec![0.5; results.len()];
    }
    let denominator = (scale.len() - 1) as f64;
    results
        .iter()
        .map(|result| {
            let Some(score) = result
                .relevance_score
                .filter(|score| score.is_finite())
                .map(|score| score.clamp(0.0, 1.0))
            else {
                return 0.5;
            };
            scale
                .binary_search_by(|candidate| candidate.total_cmp(&score))
                .map(|index| index as f64 / denominator)
                .unwrap_or(0.5)
        })
        .collect()
}

fn normalized_weight(value: f64, default: f64) -> f64 {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        value
    } else {
        default
    }
}

fn normalized_unit_interval(value: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_relevance_is_calibrated_only_inside_one_provider_response() {
        let narrow = vec![
            SearchResult::new("https://high.example", "High", "High").with_relevance_score(0.22),
            SearchResult::new("https://low.example", "Low", "Low").with_relevance_score(0.18),
        ];
        let wide = vec![
            SearchResult::new("https://high.example", "High", "High").with_relevance_score(0.98),
            SearchResult::new("https://low.example", "Low", "Low").with_relevance_score(0.12),
        ];

        assert_eq!(calibrated_native_relevance(&narrow), vec![1.0, 0.0]);
        assert_eq!(
            calibrated_native_relevance(&narrow),
            calibrated_native_relevance(&wide)
        );
    }

    #[test]
    fn absent_or_single_native_scores_are_neutral() {
        let absent = vec![SearchResult::new("https://none.example", "None", "None")];
        let single = vec![
            SearchResult::new("https://single.example", "Single", "Single")
                .with_relevance_score(0.99),
        ];

        assert_eq!(calibrated_native_relevance(&absent), vec![0.5]);
        assert_eq!(calibrated_native_relevance(&single), vec![0.5]);
    }

    #[test]
    fn invalid_policy_values_fall_back_as_one_policy() {
        let invalid = RankingConfig {
            rrf_rank_constant: f64::INFINITY,
            native_relevance_weight: f64::NAN,
        };

        assert_eq!(invalid.sanitized(), RankingConfig::default());
    }
}

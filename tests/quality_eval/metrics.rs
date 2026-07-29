use std::collections::{HashMap, HashSet};

use a3s_search::{SearchResult, SearchResults};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(super) struct CaseMetrics {
    pub case_id: String,
    pub ndcg_at_k: f64,
    pub reciprocal_rank_at_grade_2: f64,
    pub recall_at_k: f64,
    pub unique_host_ratio_at_k: f64,
    pub consensus_ratio_at_k: f64,
    pub duplicate_ratio_at_k: f64,
}

impl CaseMetrics {
    pub(super) fn assert_finite(&self) {
        for value in [
            self.ndcg_at_k,
            self.reciprocal_rank_at_grade_2,
            self.recall_at_k,
            self.unique_host_ratio_at_k,
            self.consensus_ratio_at_k,
            self.duplicate_ratio_at_k,
        ] {
            assert!(value.is_finite());
            assert!((0.0..=1.0).contains(&value));
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AggregateMetrics {
    pub case_count: usize,
    pub mean_ndcg_at_k: f64,
    pub mean_reciprocal_rank_at_grade_2: f64,
    pub mean_recall_at_k: f64,
    pub mean_unique_host_ratio_at_k: f64,
    pub mean_consensus_ratio_at_k: f64,
    pub mean_duplicate_ratio_at_k: f64,
}

impl AggregateMetrics {
    pub(super) fn from_cases(cases: &[CaseMetrics]) -> Self {
        assert!(!cases.is_empty());
        let mean = |value: fn(&CaseMetrics) -> f64| {
            cases.iter().map(value).sum::<f64>() / cases.len() as f64
        };
        Self {
            case_count: cases.len(),
            mean_ndcg_at_k: mean(|case| case.ndcg_at_k),
            mean_reciprocal_rank_at_grade_2: mean(|case| case.reciprocal_rank_at_grade_2),
            mean_recall_at_k: mean(|case| case.recall_at_k),
            mean_unique_host_ratio_at_k: mean(|case| case.unique_host_ratio_at_k),
            mean_consensus_ratio_at_k: mean(|case| case.consensus_ratio_at_k),
            mean_duplicate_ratio_at_k: mean(|case| case.duplicate_ratio_at_k),
        }
    }
}

pub(super) fn evaluate_case(
    case_id: &str,
    results: &SearchResults,
    judgments: impl IntoIterator<Item = (String, u8)>,
    k: usize,
) -> CaseMetrics {
    let judgments = judgments.into_iter().collect::<HashMap<_, _>>();
    let top = results.items().iter().take(k).collect::<Vec<_>>();
    let grades = top
        .iter()
        .map(|result| {
            judgments
                .get(&result.normalized_url())
                .copied()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let mut ideal = judgments.values().copied().collect::<Vec<_>>();
    ideal.sort_unstable_by(|left, right| right.cmp(left));
    ideal.truncate(k);

    let relevant_total = judgments.values().filter(|grade| **grade > 0).count();
    let relevant_retrieved = top
        .iter()
        .filter(|result| {
            judgments
                .get(&result.normalized_url())
                .is_some_and(|grade| *grade > 0)
        })
        .count();
    let first_material = grades.iter().position(|grade| *grade >= 2);
    let hosts = top.iter().filter_map(result_host).collect::<HashSet<_>>();
    let normalized_urls = top
        .iter()
        .map(|result| result.normalized_url())
        .collect::<Vec<_>>();
    let unique_urls = normalized_urls.iter().collect::<HashSet<_>>().len();

    CaseMetrics {
        case_id: case_id.to_string(),
        ndcg_at_k: normalized_dcg(&grades, &ideal),
        reciprocal_rank_at_grade_2: first_material
            .map(|index| 1.0 / (index + 1) as f64)
            .unwrap_or(0.0),
        recall_at_k: ratio(relevant_retrieved, relevant_total),
        unique_host_ratio_at_k: ratio(hosts.len(), top.len()),
        consensus_ratio_at_k: ratio(
            top.iter()
                .filter(|result| result.engines.len() >= 2)
                .count(),
            top.len(),
        ),
        duplicate_ratio_at_k: ratio(top.len().saturating_sub(unique_urls), top.len()),
    }
}

fn normalized_dcg(grades: &[u8], ideal: &[u8]) -> f64 {
    let ideal_dcg = discounted_cumulative_gain(ideal);
    if ideal_dcg == 0.0 {
        0.0
    } else {
        discounted_cumulative_gain(grades) / ideal_dcg
    }
}

fn discounted_cumulative_gain(grades: &[u8]) -> f64 {
    grades
        .iter()
        .enumerate()
        .map(|(index, grade)| {
            let gain = 2_f64.powi(i32::from(*grade)) - 1.0;
            gain / (index as f64 + 2.0).log2()
        })
        .sum()
}

fn result_host(result: &&SearchResult) -> Option<String> {
    url::Url::parse(&result.url)
        .ok()?
        .host_str()
        .map(|host| host.trim_start_matches("www.").to_ascii_lowercase())
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

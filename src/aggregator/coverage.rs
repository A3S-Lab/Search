//! Query-set coverage reranking over an already fused candidate list.

use std::collections::HashSet;

use crate::evidence::QueryEvidence;
use crate::SearchResult;

struct Candidate {
    result: SearchResult,
    matching_atoms: Vec<usize>,
    query_alignment: f64,
    normalized_host: Option<String>,
    original_rank: usize,
}

/// Greedily promotes complementary visible evidence without changing scores.
///
/// The strongest base-ranked result remains first. Later positions prefer the
/// candidate that adds the most uncovered query evidence, then use local
/// unseen-host evidence, local original-query alignment, base score, and
/// original rank as deterministic tie-breakers. This lets independent coherent
/// evidence replace a repeated-host weak row without changing either result's
/// score.
pub(crate) fn rerank_for_query(query: &str, results: &mut Vec<SearchResult>) {
    let evidence = QueryEvidence::new(query);
    if !evidence.is_composite() || results.len() < 2 {
        return;
    }

    let mut remaining = std::mem::take(results)
        .into_iter()
        .enumerate()
        .map(|(original_rank, result)| {
            let query_alignment = result
                .query_match_score
                .filter(|alignment| alignment.is_finite())
                .map(|alignment| alignment.clamp(0.0, 1.0))
                .unwrap_or_else(|| crate::query_match_score(query, &result));
            Candidate {
                matching_atoms: evidence.matching_atoms(&result),
                query_alignment,
                normalized_host: normalized_host(&result),
                result,
                original_rank,
            }
        })
        .collect::<Vec<_>>();
    let mut covered = HashSet::new();
    let mut covered_hosts = HashSet::new();

    let first = remaining.remove(0);
    covered.extend(first.matching_atoms.iter().copied());
    covered_hosts.extend(first.normalized_host.iter().cloned());
    results.push(first.result);

    while !remaining.is_empty() {
        let mut selected = 0usize;
        let mut selected_marginal =
            evidence.marginal_coverage(&remaining[0].matching_atoms, &covered);
        for (index, candidate) in remaining.iter().enumerate().skip(1) {
            let marginal = evidence.marginal_coverage(&candidate.matching_atoms, &covered);
            let selected_candidate = &remaining[selected];
            let host_is_new = candidate
                .normalized_host
                .as_ref()
                .is_some_and(|host| !covered_hosts.contains(host));
            let selected_host_is_new = selected_candidate
                .normalized_host
                .as_ref()
                .is_some_and(|host| !covered_hosts.contains(host));
            let ordering = marginal
                .total_cmp(&selected_marginal)
                .then_with(|| host_is_new.cmp(&selected_host_is_new))
                .then_with(|| {
                    candidate
                        .query_alignment
                        .total_cmp(&selected_candidate.query_alignment)
                })
                .then_with(|| {
                    candidate
                        .result
                        .score
                        .total_cmp(&selected_candidate.result.score)
                })
                .then_with(|| {
                    selected_candidate
                        .original_rank
                        .cmp(&candidate.original_rank)
                });
            if ordering.is_gt() {
                selected = index;
                selected_marginal = marginal;
            }
        }

        let candidate = remaining.remove(selected);
        covered.extend(candidate.matching_atoms.iter().copied());
        covered_hosts.extend(candidate.normalized_host.iter().cloned());
        results.push(candidate.result);
    }
}

fn normalized_host(result: &SearchResult) -> Option<String> {
    let url = url::Url::parse(result.url.trim()).ok()?;
    let host = url.host_str()?.trim_start_matches("www.");
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored(url: &str, title: &str, content: &str, score: f64) -> SearchResult {
        let mut result = SearchResult::new(url, title, content);
        result.score = score;
        result
    }

    #[test]
    fn complementary_evidence_enters_the_ranked_head() {
        let query = "battery storage cost safety warranty recycling";
        let mut results = (0..5)
            .map(|index| {
                scored(
                    &format!("https://partial-{index}.example/report"),
                    "Battery storage cost report",
                    "Battery storage cost estimates.",
                    1.0 - index as f64 * 0.01,
                )
            })
            .collect::<Vec<_>>();
        results.push(scored(
            "https://complement.example/evidence",
            "Storage safety evidence",
            "Warranty obligations and recycling requirements.",
            0.75,
        ));

        rerank_for_query(query, &mut results);

        assert!(
            results
                .iter()
                .take(5)
                .any(|result| result.url.contains("complement")),
            "complementary evidence should not remain behind repeated partial rows"
        );
        assert!(
            results[0].url.contains("partial"),
            "the strongest base-ranked evidence remains authoritative"
        );
    }

    #[test]
    fn reranking_is_deterministic_for_equal_utility() {
        let mut results = vec![
            scored("https://b.example", "Alpha evidence", "", 1.0),
            scored("https://a.example", "Beta evidence", "", 1.0),
        ];

        rerank_for_query("alpha beta evidence", &mut results);

        assert_eq!(results[0].url, "https://b.example");
    }

    #[test]
    fn equal_marginal_coverage_prefers_locally_coherent_evidence() {
        let query = "alpha beta gamma delta epsilon zeta";
        let mut results = vec![
            scored(
                "https://first.example/report",
                "alpha beta gamma delta",
                "alpha beta gamma delta",
                1.0,
            ),
            scored(
                "https://weak-gap.example/report",
                "epsilon zeta",
                "epsilon zeta",
                0.99,
            ),
            scored(
                "https://repeat-1.example/report",
                "alpha beta gamma delta",
                "alpha beta gamma delta",
                0.98,
            ),
            scored(
                "https://repeat-2.example/report",
                "alpha beta gamma delta",
                "alpha beta gamma delta",
                0.97,
            ),
            scored(
                "https://repeat-3.example/report",
                "alpha beta gamma delta",
                "alpha beta gamma delta",
                0.96,
            ),
            scored(
                "https://coherent.example/report",
                "alpha beta gamma delta epsilon zeta",
                "alpha beta gamma delta epsilon zeta",
                0.70,
            ),
        ];

        rerank_for_query(query, &mut results);

        assert_eq!(results[0].url, "https://first.example/report");
        assert_eq!(results[1].url, "https://coherent.example/report");
    }

    #[test]
    fn equal_evidence_utility_prefers_an_unseen_host() {
        let query = "alpha beta gamma";
        let mut results = vec![
            scored("https://dominant.example/first", query, query, 1.0),
            scored("https://dominant.example/second", query, query, 0.99),
            scored("https://dominant.example/third", query, query, 0.98),
            scored("https://independent.example/report", query, query, 0.70),
        ];

        rerank_for_query(query, &mut results);

        assert_eq!(results[0].url, "https://dominant.example/first");
        assert_eq!(results[1].url, "https://independent.example/report");
    }
}

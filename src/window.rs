//! Structurally diverse caller-visible result windows.

use std::collections::HashMap;

use crate::cascade::normalized_usable_host;
use crate::{RetrievalRequirements, SearchResult, SearchResults};

const MAX_EXACT_WINDOW_STATES: usize = 250_000;

/// Selects at most `limit` ranked results and makes a bounded attempt to preserve
/// structural retrieval requirements from the combined candidate set.
///
/// The selector does not inspect the query, title, snippet, language, publisher,
/// or topic. Results retain their relative rank after selection. If the bounded
/// exact search is exhausted, the selector applies monotonic structural
/// improvements without expanding the output limit.
pub fn select_structural_window(
    results: &SearchResults,
    limit: usize,
    requirements: RetrievalRequirements,
) -> Vec<&SearchResult> {
    let window_len = limit.min(results.items().len());
    let mut selected = (0..window_len).collect::<Vec<_>>();
    if selected.is_empty() {
        return Vec::new();
    }
    let rows = structural_rows(results.items());

    if let Some(exact) =
        exact_structural_window(&rows, window_len, requirements, MAX_EXACT_WINDOW_STATES)
    {
        return exact
            .into_iter()
            .map(|index| &results.items()[index])
            .collect();
    }

    // A bounded local improvement remains useful for unusually large windows
    // whose exact replacement state space exceeds the public safety bound.
    loop {
        let current_health = observe_window(&rows, &selected);
        let current_deficit = structural_deficit(requirements, current_health);
        if current_deficit == 0 {
            break;
        }

        let mut best_swap = None;
        let mut best_deficit = current_deficit;
        for incoming in 0..results.items().len() {
            if selected.contains(&incoming) {
                continue;
            }
            for outgoing_position in (0..selected.len()).rev() {
                let mut trial = selected.clone();
                trial[outgoing_position] = incoming;
                let health = observe_window(&rows, &trial);
                let deficit = structural_deficit(requirements, health);
                if deficit < best_deficit {
                    best_deficit = deficit;
                    best_swap = Some((outgoing_position, incoming));
                }
            }
        }

        let Some((outgoing_position, incoming)) = best_swap else {
            break;
        };
        selected[outgoing_position] = incoming;
    }

    selected.sort_unstable();
    selected
        .into_iter()
        .map(|index| &results.items()[index])
        .collect()
}

fn exact_structural_window(
    rows: &[StructuralRow],
    window_len: usize,
    requirements: RetrievalRequirements,
    maximum_states: usize,
) -> Option<Vec<usize>> {
    let initial = (0..window_len).collect::<Vec<_>>();
    let initial_health = observe_window(rows, &initial);
    if requirements_met(requirements, initial_health) {
        return Some(initial);
    }

    let outside_len = rows.len().saturating_sub(window_len);
    let maximum_replacements = window_len.min(outside_len);
    let mut observed_states = 0usize;
    for replacement_count in 1..=maximum_replacements {
        for incoming in Combinations::new(outside_len, replacement_count) {
            for outgoing in Combinations::new(window_len, replacement_count) {
                observed_states = observed_states.saturating_add(1);
                if observed_states > maximum_states {
                    return None;
                }
                let mut selected = initial.clone();
                for (outgoing, incoming) in outgoing.iter().zip(&incoming) {
                    selected[window_len - 1 - *outgoing] = window_len + *incoming;
                }
                let health = observe_window(rows, &selected);
                if requirements_met(requirements, health) {
                    selected.sort_unstable();
                    return Some(selected);
                }
            }
        }
    }
    None
}

struct Combinations {
    n: usize,
    current: Option<Vec<usize>>,
}

impl Combinations {
    fn new(n: usize, k: usize) -> Self {
        Self {
            n,
            current: (k <= n).then(|| (0..k).collect()),
        }
    }
}

impl Iterator for Combinations {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.current.clone()?;
        let current = self.current.as_mut()?;
        let k = current.len();
        let Some(index) = (0..k)
            .rev()
            .find(|index| current[*index] < self.n - k + *index)
        else {
            self.current = None;
            return Some(result);
        };
        current[index] += 1;
        for next in index + 1..k {
            current[next] = current[next - 1] + 1;
        }
        Some(result)
    }
}

fn structural_deficit(requirements: RetrievalRequirements, health: WindowHealth) -> usize {
    requirements
        .min_usable_results
        .saturating_sub(health.usable_result_count)
        .saturating_add(
            requirements
                .min_unique_hosts
                .saturating_sub(health.unique_host_count),
        )
        .saturating_add(
            requirements
                .min_contributing_engines
                .saturating_sub(health.contributing_engine_count),
        )
        .saturating_add(
            requirements
                .min_consensus_results
                .saturating_sub(health.consensus_result_count),
        )
}

#[derive(Clone)]
struct StructuralRow {
    host: Option<usize>,
    engines: Vec<usize>,
    consensus: bool,
}

#[derive(Clone, Copy)]
struct WindowHealth {
    usable_result_count: usize,
    unique_host_count: usize,
    contributing_engine_count: usize,
    consensus_result_count: usize,
}

fn structural_rows(results: &[SearchResult]) -> Vec<StructuralRow> {
    let mut hosts = HashMap::<String, usize>::new();
    let mut engines = HashMap::<String, usize>::new();
    results
        .iter()
        .map(|result| {
            let host = normalized_usable_host(result).map(|host| {
                let next = hosts.len();
                *hosts.entry(host).or_insert(next)
            });
            let engine_ids = result
                .engines
                .iter()
                .map(|engine| {
                    let next = engines.len();
                    *engines.entry(engine.clone()).or_insert(next)
                })
                .collect::<Vec<_>>();
            StructuralRow {
                host,
                consensus: engine_ids.len() >= 2,
                engines: engine_ids,
            }
        })
        .collect()
}

fn observe_window(rows: &[StructuralRow], selected: &[usize]) -> WindowHealth {
    let mut hosts = Vec::new();
    let mut engines = Vec::new();
    let mut health = WindowHealth {
        usable_result_count: 0,
        unique_host_count: 0,
        contributing_engine_count: 0,
        consensus_result_count: 0,
    };
    for index in selected {
        let row = &rows[*index];
        let Some(host) = row.host else {
            continue;
        };
        health.usable_result_count = health.usable_result_count.saturating_add(1);
        if !hosts.contains(&host) {
            hosts.push(host);
        }
        for engine in &row.engines {
            if !engines.contains(engine) {
                engines.push(*engine);
            }
        }
        if row.consensus {
            health.consensus_result_count = health.consensus_result_count.saturating_add(1);
        }
    }
    health.unique_host_count = hosts.len();
    health.contributing_engine_count = engines.len();
    health
}

fn requirements_met(requirements: RetrievalRequirements, health: WindowHealth) -> bool {
    health.usable_result_count >= requirements.min_usable_results
        && health.unique_host_count >= requirements.min_unique_hosts
        && health.contributing_engine_count >= requirements.min_contributing_engines
        && health.consensus_result_count >= requirements.min_consensus_results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetrievalHealth;

    fn result(url: &str, engine: &str) -> SearchResult {
        SearchResult::new(url, url, "opaque").with_engine(engine, 1)
    }

    fn urls<'a>(results: &[&'a SearchResult]) -> Vec<&'a str> {
        results.iter().map(|result| result.url.as_str()).collect()
    }

    #[test]
    fn an_already_healthy_prefix_is_unchanged() {
        let mut results = SearchResults::new();
        results.add_result(result("https://one.example/1", "first"));
        results.add_result(result("https://two.example/2", "second"));
        results.add_result(result("https://three.example/3", "first"));
        results.add_result(result("https://four.example/4", "second"));
        results.add_result(result("https://five.example/5", "first"));
        results.add_result(result("https://six.example/6", "third"));

        let selected = select_structural_window(&results, 5, RetrievalRequirements::for_limit(5));

        assert_eq!(
            urls(&selected),
            vec![
                "https://one.example/1",
                "https://two.example/2",
                "https://three.example/3",
                "https://four.example/4",
                "https://five.example/5",
            ]
        );
    }

    #[test]
    fn a_lower_ranked_host_replaces_only_the_lowest_redundant_row() {
        let mut results = SearchResults::new();
        results.add_result(result("https://one.example/1", "first"));
        results.add_result(result("https://one.example/2", "first"));
        results.add_result(result("https://two.example/3", "second"));
        results.add_result(result("https://two.example/4", "second"));
        results.add_result(result("https://two.example/5", "second"));
        results.add_result(result("https://three.example/6", "first"));

        let requirements = RetrievalRequirements::for_limit(5);
        assert!(!requirements.is_met(&requirements.evaluate_items(results.items().iter().take(5))));

        let selected = select_structural_window(&results, 5, requirements);

        assert_eq!(
            urls(&selected),
            vec![
                "https://one.example/1",
                "https://one.example/2",
                "https://two.example/3",
                "https://two.example/4",
                "https://three.example/6",
            ]
        );
        assert!(requirements.is_met(&requirements.evaluate_items(selected.iter().copied())));
    }

    #[test]
    fn a_lower_ranked_logical_source_can_satisfy_the_visible_quorum() {
        let mut results = SearchResults::new();
        for index in 1..=5 {
            results.add_result(result(&format!("https://host-{index}.example/"), "first"));
        }
        results.add_result(result("https://host-6.example/", "second"));

        let requirements = RetrievalRequirements::for_limit(5);
        let selected = select_structural_window(&results, 5, requirements);

        assert_eq!(selected.last().unwrap().url, "https://host-6.example/");
        assert!(requirements.is_met(&requirements.evaluate_items(selected.iter().copied())));
    }

    #[test]
    fn selection_is_best_effort_when_the_candidate_set_cannot_meet_the_floor() {
        let mut results = SearchResults::new();
        results.add_result(result("https://one.example/1", "only"));
        results.add_result(result("https://one.example/2", "only"));
        results.add_result(result("https://two.example/3", "only"));

        let requirements = RetrievalRequirements::for_limit(3);
        let selected = select_structural_window(&results, 3, requirements);

        assert_eq!(urls(&selected).len(), 3);
        assert_eq!(urls(&selected)[0], "https://one.example/1");
        assert!(!requirements.is_met(&requirements.evaluate_items(selected.iter().copied())));
    }

    #[test]
    fn selection_can_cross_an_equal_deficit_intermediate_window() {
        let mut results = SearchResults::new();
        results.add_result(result("https://one.example/1", "first"));
        results.add_result(result("https://zero.example/2", "third"));
        results.add_result(result("https://one.example/3", "second").with_engine("third", 2));
        results.add_result(result("https://zero.example/4", "first").with_engine("third", 2));
        results.add_result(result("https://three.example/5", "third"));
        let requirements = RetrievalRequirements {
            min_usable_results: 3,
            min_unique_hosts: 3,
            min_contributing_engines: 3,
            min_consensus_results: 1,
        };

        let selected = select_structural_window(&results, 3, requirements);

        assert_eq!(
            urls(&selected),
            vec![
                "https://one.example/3",
                "https://zero.example/4",
                "https://three.example/5",
            ]
        );
        assert!(requirements.is_met(&requirements.evaluate_items(selected.iter().copied())));
    }

    #[test]
    fn precomputed_window_health_matches_the_public_observer() {
        let results = vec![
            result("https://www.one.example/1", "first"),
            result("https://two.example/2", "second").with_engine("first", 2),
            result("not-a-url", "third"),
        ];
        let rows = structural_rows(&results);
        let internal = observe_window(&rows, &[0, 1, 2]);
        let public = RetrievalHealth::observe_items(results.iter());

        assert_eq!(internal.usable_result_count, public.usable_result_count);
        assert_eq!(internal.unique_host_count, public.unique_host_count);
        assert_eq!(
            internal.contributing_engine_count,
            public.contributing_engine_count
        );
        assert_eq!(
            internal.consensus_result_count,
            public.consensus_result_count
        );
    }
}

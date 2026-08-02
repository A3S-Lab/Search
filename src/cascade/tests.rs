use super::*;
use crate::SearchResult;

fn result(url: &str, engine: &str) -> SearchResult {
    SearchResult::new(url, "opaque title", "opaque snippet").with_engine(engine, 1)
}

#[test]
fn health_is_independent_of_query_and_result_text() {
    let mut first = SearchResults::new();
    first.add_result(result("https://one.example/report", "first"));
    let mut second = first.clone();
    second.items_mut()[0].title = "completely different text".to_string();
    second.items_mut()[0].content = "unrelated content in another language".to_string();
    second.items_mut()[0].full_text = Some("arbitrary body".to_string());

    assert_eq!(
        RetrievalHealth::observe(&first),
        RetrievalHealth::observe(&second)
    );
}

#[test]
fn health_reports_only_structure_provenance_and_typed_outcomes() {
    let mut results = SearchResults::new();
    results.add_result(result("https://www.one.example/report", "first"));
    results.add_result(result("https://two.example/report", "first").with_engine("second", 2));
    results.add_result(SearchResult::new("not a URL", "title", "snippet"));

    let health = RetrievalHealth::observe(&results);

    assert_eq!(health.usable_result_count, 2);
    assert_eq!(health.invalid_result_count, 1);
    assert_eq!(health.unique_host_count, 2);
    assert_eq!(health.contributing_engine_count, 2);
    assert_eq!(health.consensus_result_count, 1);
}

#[test]
fn default_cascade_uses_only_structural_requirements() {
    let requirements = RetrievalRequirements {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_consensus_results: 0,
    };
    let mut cascade = SearchCascade::new(SearchQuery::new("opaque query"), requirements);
    let mut first = SearchResults::new();
    first.add_result(result("https://one.example", "source"));
    assert_eq!(
        cascade.push_tier("first", first),
        SearchTierDecision::Continue
    );

    let mut second = SearchResults::new();
    second.add_result(result("https://two.example", "source"));
    assert_eq!(
        cascade.push_tier("second", second),
        SearchTierDecision::Stop
    );
}

#[test]
fn external_policy_can_continue_after_structural_requirements_are_met() {
    let mut cascade = SearchCascade::new(
        SearchQuery::new("opaque query"),
        RetrievalRequirements::for_limit(1),
    );
    let mut results = SearchResults::new();
    results.add_result(result("https://one.example", "source"));

    assert_eq!(
        cascade.push_tier_with_decision("first", results, SearchTierDecision::Continue),
        SearchTierDecision::Continue
    );
    assert!(cascade
        .requirements()
        .is_met(&RetrievalHealth::observe(cascade.results())));
    assert!(cascade.needs_next_tier());
    assert_eq!(
        cascade.reports()[0].decision_source,
        SearchTierDecisionSource::ExternalPolicy
    );
}

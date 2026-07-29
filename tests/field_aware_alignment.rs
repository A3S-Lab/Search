//! Field-invariant checks for domain-neutral query alignment.

use a3s_search::{query_match_score, Aggregator, SearchQuality, SearchQualityFloor, SearchResult};

fn result(url: impl Into<String>, title: &str, content: &str) -> SearchResult {
    SearchResult::new(url, title, content)
}

#[test]
fn url_query_echoes_cannot_stop_fallback_while_visible_result_text_can() {
    let query = "distributed tracing sampling specification";
    let floor = SearchQualityFloor::for_limit(3);
    let aggregate = |results| {
        Aggregator::new().aggregate_for_query(query, vec![("opaque-source".to_string(), results)])
    };

    let url_echoes = aggregate(
        ["one", "two", "three"]
            .into_iter()
            .map(|host| {
                result(
                    format!("https://{host}.invalid/distributed-tracing-sampling-specification"),
                    "",
                    "",
                )
            })
            .collect(),
    );
    let echo_quality = SearchQuality::evaluate(query, &url_echoes, floor.min_query_match);
    assert_eq!(echo_quality.usable_result_count, 3);
    assert_eq!(echo_quality.unique_host_count, 3);
    assert_eq!(echo_quality.aligned_result_count, 0);
    assert!(!floor.is_met(&echo_quality));

    let visible_results = aggregate(
        ["four", "five", "six"]
            .into_iter()
            .map(|host| {
                result(
                    format!("https://{host}.invalid/document"),
                    "",
                    "Distributed tracing sampling specification",
                )
            })
            .collect(),
    );
    let visible_quality = SearchQuality::evaluate(query, &visible_results, floor.min_query_match);
    assert_eq!(visible_quality.aligned_result_count, 3);
    assert!(floor.is_met(&visible_quality));
}

#[test]
fn visible_result_text_outranks_url_echoes_across_scripts() {
    let cases = [
        "municipal heat pump lifecycle costs",
        "城市防洪标准最新实施日期",
        "متطلبات سلامة تخزين الهيدروجين",
        "港湾物流自動化リスク分析",
        "ग्रामीण जल कार्यक्रम प्रभाव मूल्यांकन",
    ];

    for (index, query) in cases.into_iter().enumerate() {
        let url_echo = result(format!("https://echo-{index}.invalid/{query}"), "", "");
        let visible_result = result(
            format!("https://evidence-{index}.invalid/document"),
            "",
            query,
        );

        let echo_score = query_match_score(query, &url_echo);
        let visible_score = query_match_score(query, &visible_result);
        assert!(
            visible_score > echo_score,
            "visible result text did not outrank a URL-only echo for case {index}: {visible_score} <= {echo_score}"
        );

        let ranked = Aggregator::new().aggregate_for_query(
            query,
            vec![("opaque-source".to_string(), vec![url_echo, visible_result])],
        );
        assert_eq!(
            ranked.items()[0].url,
            format!("https://evidence-{index}.invalid/document")
        );
    }
}

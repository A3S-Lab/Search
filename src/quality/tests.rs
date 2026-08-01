use super::*;
use crate::Aggregator;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn result(url: &str, title: &str, content: &str) -> SearchResult {
    SearchResult::new(url, title, content)
}

#[test]
fn query_alignment_is_domain_and_language_neutral() {
    let english = result(
        "https://docs.example/async-traits",
        "Async functions in traits",
        "Official language reference for async trait methods",
    );
    let english_noise = result(
        "https://example.test/game",
        "A survival game",
        "Unrelated entertainment page",
    );
    assert!(
        query_match_score("async fn in traits official reference", &english)
            > query_match_score("async fn in traits official reference", &english_noise)
    );

    let chinese = result(
        "https://example.cn/transport",
        "跨境交通运行评估",
        "这份报告披露赛事期间跨境交通的运行表现",
    );
    let chinese_noise = result(
        "https://example.cn/food",
        "城市餐饮指南",
        "介绍本地餐厅和菜单",
    );
    assert!(
        query_match_score("跨境交通运行表现报告", &chinese)
            > query_match_score("跨境交通运行表现报告", &chinese_noise)
    );
}

#[test]
fn partial_character_evidence_aligns_unsegmented_multi_clause_queries() {
    let query = "跨境交通运行表现 官方统计";
    let relevant = result(
        "https://evidence.example/transport",
        "跨境交通运行年度统计",
        "公共机构发布跨境交通运行表现数据",
    );
    let noise = result(
        "https://noise.example/dining",
        "城市餐饮年度榜单",
        "介绍本地餐厅和季节菜单",
    );

    let relevant_score = query_match_score(query, &relevant);
    assert!(relevant_score >= 0.35, "observed {relevant_score}");
    assert!(relevant_score > query_match_score(query, &noise));
}

#[test]
fn multi_term_alignment_combines_title_and_snippet_evidence() {
    let query = "battery storage cost safety warranty recycling";
    let distributed = result(
        "https://evidence.example/storage",
        "Battery storage cost and safety evidence",
        "Warranty obligations and recycling requirements.",
    );
    let repeated_partial = result(
        "https://partial.example/storage",
        "Battery storage cost report",
        "More battery storage cost estimates.",
    );

    let distributed_score = query_match_score(query, &distributed);
    assert!(distributed_score >= 0.35, "observed {distributed_score}");
    assert!(distributed_score > query_match_score(query, &repeated_partial));
}

#[test]
fn character_evidence_recovers_inflected_terms_in_long_questions() {
    let query =
        "What evidence explains why coastal barriers failed while inland defenses survived?";
    let relevant = result(
        "https://evidence.example/defenses",
        "Why coastal barriers fail while inland defenses survive",
        "Evidence explaining the different outcomes and their limits.",
    );
    let noise = result(
        "https://noise.example/coast",
        "Coastal travel guide",
        "Restaurants, hotels, and seasonal events.",
    );

    let relevant_score = query_match_score(query, &relevant);
    assert!(relevant_score >= 0.35, "observed {relevant_score}");
    assert!(relevant_score > query_match_score(query, &noise));
}

#[test]
fn unsegmented_composite_queries_match_distributed_concepts_without_accepting_noise() {
    let cases = [
        (
            "工业储能成本收益风险证据",
            result(
                "https://evidence.example/storage",
                "工业储能项目评估",
                "研究分别说明储能成本、预期收益、实施风险和支持证据。",
            ),
            result(
                "https://noise.example/industry",
                "工业项目年度目录",
                "汇总企业名称、地址和联系电话。",
            ),
        ),
        (
            "港湾自動化便益リスク証拠",
            result(
                "https://evidence.example/port",
                "港湾自動化の評価",
                "導入の便益、運用リスク、費用と検証証拠を整理する。",
            ),
            result(
                "https://noise.example/port",
                "港湾の観光案内",
                "飲食店と季節イベントを紹介する。",
            ),
        ),
        (
            "산업배터리비용편익위험근거",
            result(
                "https://evidence.example/battery",
                "산업 배터리 사업 평가",
                "비용, 편익, 운영 위험과 판단 근거를 각각 검토한다.",
            ),
            result(
                "https://noise.example/battery",
                "산업 행사 일정",
                "전시장 위치와 방문 시간을 안내한다.",
            ),
        ),
    ];

    for (query, relevant, noise) in cases {
        let relevant_score = query_match_score(query, &relevant);
        let noise_score = query_match_score(query, &noise);
        assert!(
            relevant_score >= 0.35,
            "{query}: distributed evidence scored {relevant_score}"
        );
        assert!(
            noise_score < 0.18,
            "{query}: unrelated material scored {noise_score}"
        );
    }
}

#[test]
fn mixed_script_queries_require_visible_evidence_from_each_substantive_script() {
    let query = "WebTransport 与 HTTP/3 迁移差异";
    let native_context = result(
        "https://evidence.example/web-transport",
        "WebTransport 与 HTTP/3 的迁移差异",
        "中文说明连接迁移、双向传输和兼容性边界。",
    );
    let other_script_only = result(
        "https://partial.example/web-transport",
        "WebTransport and HTTP/3 migration differences",
        "An English comparison of connection migration and compatibility.",
    );

    let native_score = query_match_score(query, &native_context);
    let partial_score = query_match_score(query, &other_script_only);
    assert!(native_score >= 0.35, "native context scored {native_score}");
    assert!(
        partial_score < 0.35,
        "missing-script evidence scored {partial_score}"
    );
}

#[test]
fn multi_term_alignment_rejects_generic_word_and_boilerplate_matches() {
    let query = "global renewable energy outlook IEA IRENA World Bank policy reports";
    let generic = result(
        "https://dictionary.example/global",
        "GLOBAL Definition & Meaning",
        "A word used for the whole world. Send a report if this entry has a problem.",
    );
    let specific = result(
        "https://evidence.example/world-energy-outlook",
        "World Energy Outlook - IEA",
        "Renewable energy policy report with IRENA and World Bank evidence.",
    );

    assert!(query_match_score(query, &generic) < 0.18);
    assert!(query_match_score(query, &specific) >= 0.18);
}

#[test]
fn default_floor_rejects_partial_capacity_and_weak_set_averages() {
    let query = "distributed tracing baggage propagation sampling specification";
    let floor = SearchQualityFloor::for_limit(5);
    let aggregate =
        |results| Aggregator::new().aggregate_for_query(query, vec![("api".to_string(), results)]);
    let urls = [
        "https://one.example/article",
        "https://two.example/article",
        "https://three.example/article",
        "https://four.example/article",
        "https://five.example/article",
    ];

    let shallow = aggregate(
        urls.iter()
            .map(|url| {
                result(
                    url,
                    "Distributed tracing overview",
                    "An introduction to distributed tracing",
                )
            })
            .collect(),
    );
    let shallow_quality = SearchQuality::evaluate(query, &shallow, floor.min_query_match);
    assert_eq!(shallow_quality.usable_result_count, 5);
    assert_eq!(shallow_quality.unique_host_count, 5);
    assert_eq!(shallow_quality.aligned_result_count, 0);
    assert!(!floor.is_met(&shallow_quality));

    let mixed = aggregate(
        urls.iter()
            .enumerate()
            .map(|(index, url)| {
                if index < 3 {
                    result(
                        url,
                        "Distributed tracing baggage",
                        "Distributed tracing baggage guidance",
                    )
                } else {
                    result(url, "Unrelated article", "General introduction")
                }
            })
            .collect(),
    );
    let mixed_quality = SearchQuality::evaluate(query, &mixed, floor.min_query_match);
    assert_eq!(
        mixed_quality.aligned_result_count, 0,
        "repeated partial rows do not cover the ranked evidence window"
    );
    assert!(mixed_quality.mean_query_match < floor.min_mean_query_match);
    assert!(!floor.is_met(&mixed_quality));

    let strong = aggregate(
        urls.iter()
            .map(|url| {
                result(
                    url,
                    "Distributed tracing baggage propagation specification",
                    "Sampling requirements for distributed tracing baggage propagation",
                )
            })
            .collect(),
    );
    let strong_quality = SearchQuality::evaluate(query, &strong, floor.min_query_match);
    assert!(floor.is_met(&strong_quality));
}

#[test]
fn default_floor_rejects_repeated_partial_composite_intent() {
    let query = "battery storage cost safety warranty recycling";
    let floor = SearchQualityFloor::for_limit(5);
    let partial = Aggregator::new().aggregate_for_query(
        query,
        vec![(
            "api".to_string(),
            (0..5)
                .map(|index| {
                    result(
                        &format!("https://partial-{index}.example/report"),
                        "Battery storage cost report",
                        "Battery storage cost estimates and market data.",
                    )
                })
                .collect(),
        )],
    );

    let quality = floor.evaluate(query, &partial);
    assert_eq!(quality.usable_result_count, 5);
    assert!(
        !floor.is_met(&quality),
        "repeating one subset of a composite query must not satisfy the complete evidence floor: \
         {quality:?}"
    );
}

#[test]
fn cascade_uses_complementary_tier_evidence_for_composite_intent() {
    let query = "battery storage cost safety warranty recycling";
    let floor = SearchQualityFloor::for_limit(5);
    let aggregator = Aggregator::new();
    let partial = aggregator.aggregate_for_query(
        query,
        vec![(
            "api".to_string(),
            (0..5)
                .map(|index| {
                    result(
                        &format!("https://partial-{index}.example/report"),
                        "Battery storage cost report",
                        "Battery storage cost estimates and market data.",
                    )
                })
                .collect(),
        )],
    );
    let complementary = aggregator.aggregate_for_query(
        query,
        vec![(
            "http".to_string(),
            (0..5)
                .map(|index| {
                    result(
                        &format!("https://complete-{index}.example/evidence"),
                        "Battery storage cost and safety evidence",
                        "Warranty terms, recycling obligations, and lifecycle evidence.",
                    )
                })
                .collect(),
        )],
    );
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    assert_eq!(
        cascade.push_tier("api", partial),
        SearchTierDecision::Continue
    );
    assert_eq!(
        cascade.push_tier("http", complementary),
        SearchTierDecision::Stop
    );
}

#[test]
fn disjoint_weak_rows_cannot_stitch_a_composite_query_into_a_pass() {
    let query = "alpha beta gamma delta epsilon zeta";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();
    let evidence = [
        ("alpha beta", "first pair"),
        ("gamma delta", "second pair"),
        ("epsilon zeta", "third pair"),
        ("alpha beta", "independent first pair"),
        ("gamma delta", "independent second pair"),
    ];
    for (index, (title, content)) in evidence.into_iter().enumerate() {
        let mut result = result(
            &format!("https://evidence-{index}.example/report"),
            title,
            content,
        )
        .with_engine("opaque-source", index as u32 + 1);
        result.query_match_score = Some(0.30);
        results.add_result(result);
    }

    let quality = floor.evaluate(query, &results);
    assert_eq!(floor.min_query_match, 0.35);
    assert_eq!(quality.aligned_result_count, 0);
    assert!(!floor.is_met(&quality), "{quality:?}");
}

#[test]
fn locally_aligned_rows_can_complete_composite_evidence_as_a_set() {
    let query = "alpha beta gamma delta epsilon zeta";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();
    let evidence = [
        "alpha beta gamma",
        "alpha beta delta",
        "alpha beta epsilon",
        "alpha beta zeta",
        "alpha beta gamma delta",
    ];
    for (index, content) in evidence.into_iter().enumerate() {
        results.add_result(
            result(
                &format!("https://evidence-{index}.example/report"),
                content,
                content,
            )
            .with_engine("opaque-source", index as u32 + 1),
        );
    }

    let quality = floor.evaluate(query, &results);
    assert_eq!(floor.min_query_match, 0.35);
    assert!(quality.aligned_result_count >= 3, "{quality:?}");
    assert!(floor.is_met(&quality), "{quality:?}");
}

#[test]
fn bounded_full_text_can_complete_set_evidence_without_changing_base_alignment() {
    let query = "battery storage cost safety warranty recycling";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();
    let bodies = [
        "Safety requirements and incident evidence.",
        "Warranty obligations and claim evidence.",
        "Recycling requirements and lifecycle evidence.",
        "Independent safety and warranty analysis.",
        "Independent recycling and lifecycle analysis.",
    ];

    for (index, body) in bodies.into_iter().enumerate() {
        let mut item = result(
            &format!("https://evidence-{index}.example/report"),
            "Battery storage cost report",
            "Cost estimates for battery storage projects.",
        )
        .with_engine("opaque-source", index as u32 + 1);
        let base_alignment = query_match_score(query, &item);
        item.full_text = Some(body.to_string());
        assert_eq!(
            query_match_score(query, &item),
            base_alignment,
            "full text must not alter title/snippet/URL base alignment"
        );
        results.add_result(item);
    }

    let quality = floor.evaluate(query, &results);
    assert!(floor.is_met(&quality), "{quality:?}");
}

#[test]
fn full_text_supplements_quality_without_becoming_a_base_rank_signal() {
    let query = "battery storage cost safety warranty recycling";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();

    for index in 0..5 {
        let mut item = result(
            &format!("https://evidence-{index}.example/report"),
            "Independent project report",
            "Methods and source notes.",
        )
        .with_engine("opaque-source", index as u32 + 1);
        let base_alignment = query_match_score(query, &item);
        assert!(base_alignment < floor.min_mean_query_match);
        item.full_text =
            Some("Battery storage cost, safety, warranty, and recycling evidence.".to_string());
        assert_eq!(query_match_score(query, &item), base_alignment);
        results.add_result(item);
    }

    let quality = floor.evaluate(query, &results);
    assert!(floor.is_met(&quality), "{quality:?}");
}

#[test]
fn repeated_or_unrelated_full_text_cannot_satisfy_composite_quality() {
    let query = "battery storage cost safety warranty recycling";
    let floor = SearchQualityFloor::for_limit(5);
    let evaluate = |body: &str| {
        let mut results = SearchResults::new();
        for index in 0..5 {
            let mut item = result(
                &format!("https://evidence-{index}.example/report"),
                "Battery storage cost report",
                "Cost estimates for battery storage projects.",
            )
            .with_engine("opaque-source", index as u32 + 1);
            item.full_text = Some(body.to_string());
            results.add_result(item);
        }
        floor.evaluate(query, &results)
    };

    let repeated = evaluate("Battery storage cost estimates and market data.");
    assert!(
        !floor.is_met(&repeated),
        "duplicated partial body evidence must fail closed: {repeated:?}"
    );

    let unrelated = evaluate("Seasonal restaurant guide and local event calendar.");
    assert!(
        !floor.is_met(&unrelated),
        "unrelated body evidence must fail closed: {unrelated:?}"
    );
}

#[test]
fn composite_quality_requires_evidence_for_every_query_unit() {
    let query = "QZ 719 heliostat membrane certification official results 2037";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();
    for index in 0..5 {
        let mut item = result(
            &format!("https://evidence-{index}.example/certification"),
            "Heliostat membrane certification",
            "Official certification results and supporting evidence.",
        )
        .with_engine("opaque-source", index as u32 + 1);
        item.query_match_score = Some(0.80);
        results.add_result(item);
    }

    let quality = floor.evaluate(query, &results);
    assert_eq!(quality.aligned_result_count, 0);
    assert!(
        !floor.is_met(&quality),
        "missing atomic identifier and date evidence must fail closed: {quality:?}"
    );
}

#[test]
fn connected_identifier_fragments_cannot_be_joined_across_results() {
    let query = "NX-4827 coastal sensor certification findings";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();
    let evidence = [
        "NX coastal sensor",
        "4827 coastal certification",
        "sensor certification findings",
        "coastal sensor findings",
        "certification findings",
    ];
    for (index, content) in evidence.into_iter().enumerate() {
        let mut item = result(
            &format!("https://evidence-{index}.example/report"),
            content,
            content,
        )
        .with_engine("opaque-source", index as u32 + 1);
        item.query_match_score = Some(0.80);
        results.add_result(item);
    }

    let quality = floor.evaluate(query, &results);
    assert_eq!(quality.aligned_result_count, 0);
    assert!(
        !floor.is_met(&quality),
        "separate identifier fragments must not substitute for the connected atom: {quality:?}"
    );
}

#[test]
fn single_connected_identifier_requires_complete_visible_evidence() {
    let query = "X-123456";
    let floor = SearchQualityFloor::for_limit(5);
    let mut results = SearchResults::new();
    for index in 0..5 {
        let mut item = result(
            &format!("https://evidence-{index}.example/report"),
            "123456 registry entry",
            "Numbered registry evidence without the connected prefix.",
        )
        .with_engine("opaque-source", index as u32 + 1);
        item.query_match_score = Some(0.80);
        results.add_result(item);
    }

    let quality = floor.evaluate(query, &results);
    assert_eq!(quality.aligned_result_count, 0);
    assert!(!floor.is_met(&quality), "{quality:?}");
}

#[test]
fn refinement_retains_missing_units_and_bounded_context() {
    let query = "alpha beta gamma delta epsilon";
    let mut results = SearchResults::new();
    for index in 0..5 {
        results.add_result(result(
            &format!("https://evidence-{index}.example/report"),
            "Alpha evidence",
            if index < 3 {
                "Beta analysis"
            } else {
                "General analysis"
            },
        ));
    }
    results.items_mut()[0].content.push_str(" with gamma");

    let refinement = refine_query_for_evidence(query, &results, 5).unwrap();
    assert_eq!(refinement.query, "alpha delta epsilon");
    assert_eq!(refinement.total_units, 5);
    assert_eq!(refinement.covered_units, 3);
    assert_eq!(refinement.retained_units, 3);
}

#[test]
fn refinement_portfolio_partitions_least_supported_units_across_slots() {
    let query = "alpha beta gamma delta";
    let mut results = SearchResults::new();
    for (index, content) in ["alpha beta", "alpha gamma", "alpha delta"]
        .into_iter()
        .enumerate()
    {
        results.add_result(result(
            &format!("https://evidence-{index}.example/report"),
            "Evidence",
            content,
        ));
    }

    let portfolio = refine_query_portfolio(query, &results, 3, 2);
    assert_eq!(portfolio.len(), 2);
    assert_eq!(portfolio[0].query, "alpha beta delta");
    assert_eq!(portfolio[1].query, "alpha gamma");
    assert!(portfolio.iter().all(|entry| entry.covered_units == 4));
}

#[test]
fn refinement_retains_enough_context_for_original_query_alignment() {
    let query = "alpha beta gamma delta epsilon zeta eta theta";
    let mut results = SearchResults::new();
    for index in 0..5 {
        results.add_result(result(
            &format!("https://evidence-{index}.example/report"),
            "alpha beta gamma delta",
            "epsilon zeta eta",
        ));
    }

    let refinement = refine_query_for_evidence(query, &results, 5).unwrap();

    assert!(refinement.query.contains("theta"));
    assert!(
        refinement.retained_units >= 4,
        "a long refinement must retain enough original context: {refinement:?}"
    );
}

#[test]
fn quality_floor_evaluates_the_ranked_head_without_cherry_picking_the_tail() {
    let query = "distributed tracing sampling specification";
    let floor = SearchQualityFloor::for_limit(3);
    let mut strong_head = SearchResults::new();
    for index in 0..3 {
        strong_head.add_result(
            result(
                &format!("https://evidence-{index}.example/specification"),
                "Distributed tracing sampling specification",
                "Normative propagation and sampling requirements",
            )
            .with_engine("opaque-source", index + 1),
        );
    }
    for index in 0..12 {
        strong_head.add_result(
            result(
                &format!("https://tail-{index}.example/index"),
                "General technology index",
                "Unrelated directory entry",
            )
            .with_engine("opaque-source", index + 4),
        );
    }

    let all_results = SearchQuality::evaluate(query, &strong_head, floor.min_query_match);
    assert!(all_results.mean_query_match < floor.min_mean_query_match);
    let head_quality = floor.evaluate(query, &strong_head);
    assert_eq!(head_quality.usable_result_count, 3);
    assert!(floor.is_met(&head_quality));

    let mut weak_head = SearchResults::new();
    for index in 0..3 {
        weak_head.add_result(
            result(
                &format!("https://noise-{index}.example/index"),
                "General technology index",
                "Unrelated directory entry",
            )
            .with_engine("opaque-source", index + 1),
        );
    }
    for index in 0..3 {
        weak_head.add_result(
            result(
                &format!("https://later-{index}.example/specification"),
                "Distributed tracing sampling specification",
                "Normative propagation and sampling requirements",
            )
            .with_engine("opaque-source", index + 4),
        );
    }

    assert!(!floor.is_met(&floor.evaluate(query, &weak_head)));
}

#[test]
fn quality_floor_head_accounts_for_contributing_engine_requirement() {
    let query = "distributed tracing sampling specification";
    let mut floor = SearchQualityFloor::for_limit(1);
    floor.min_contributing_engines = 3;

    let mut results = SearchResults::new();
    for index in 0..3 {
        results.add_result(
            result(
                &format!("https://evidence-{index}.example/specification"),
                "Distributed tracing sampling specification",
                "Normative propagation and sampling requirements",
            )
            .with_engine(format!("opaque-source-{index}"), 1),
        );
    }

    let quality = floor.evaluate(query, &results);
    assert_eq!(quality.usable_result_count, 3);
    assert_eq!(quality.contributing_engine_count, 3);
    assert!(floor.is_met(&quality));
}

#[test]
fn repeated_query_units_do_not_inflate_alignment_across_scripts() {
    let cases = [
        (
            "distributed tracing sampling specification",
            "distributed distributed tracing sampling specification",
            result(
                "https://example.test/tracing",
                "Distributed tracing specification",
                "Sampling semantics",
            ),
        ),
        (
            "跨境交通 运行表现 官方统计",
            "跨境交通 跨境交通 运行表现 官方统计",
            result(
                "https://example.test/transport",
                "跨境交通运行年度统计",
                "公共机构发布运行表现数据",
            ),
        ),
    ];

    for (once, repeated, evidence) in cases {
        assert_eq!(
            query_match_score(once, &evidence),
            query_match_score(repeated, &evidence)
        );
    }
}

#[test]
fn query_aware_aggregation_demotes_low_alignment_capacity() {
    let aggregator = Aggregator::new();
    let ranked = aggregator.aggregate_for_query(
        "malaria vaccine position paper",
        vec![(
            "engine".to_string(),
            vec![
                result(
                    "https://noise.example/world",
                    "World news",
                    "General headlines",
                ),
                result(
                    "https://evidence.example/malaria-vaccine",
                    "Malaria vaccine position paper",
                    "Technical recommendation and evidence review",
                ),
            ],
        )],
    );

    assert_eq!(
        ranked.items()[0].url,
        "https://evidence.example/malaria-vaccine"
    );
    assert!(
        ranked.items()[0].query_match_score.unwrap() > ranked.items()[1].query_match_score.unwrap()
    );
}

#[test]
fn cascade_runs_lower_tier_only_until_quality_floor_is_met() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = SearchQuery::new("async trait reference");
    let aggregator = Aggregator::new();
    let mut cascade = SearchCascade::new(query, floor);

    let api = aggregator.aggregate_for_query(
        "async trait reference",
        vec![(
            "api".to_string(),
            vec![result(
                "https://noise.example/",
                "General programming news",
                "A broad index",
            )],
        )],
    );
    assert_eq!(cascade.push_tier("api", api), SearchTierDecision::Continue);

    let http = aggregator.aggregate_for_query(
        "async trait reference",
        vec![(
            "http".to_string(),
            vec![
                result(
                    "https://reference.example/async-trait",
                    "Async trait reference",
                    "Language reference",
                ),
                result(
                    "https://guide.example/async-trait",
                    "Async trait guide",
                    "Reference guide",
                ),
            ],
        )],
    );
    assert_eq!(cascade.push_tier("http", http), SearchTierDecision::Stop);
    assert!(!cascade.needs_next_tier());
    assert_eq!(cascade.reports().len(), 2);
}

#[tokio::test]
async fn lazy_cascade_does_not_initialize_http_or_headless_after_api_quality() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = "distributed tracing sampling specification";
    let aggregator = Aggregator::new();
    let api_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "api".to_string(),
            vec![
                result(
                    "https://reference.example/tracing",
                    "Distributed tracing specification",
                    "Sampling semantics and propagation rules",
                ),
                result(
                    "https://guide.example/sampling",
                    "Tracing sampling guide",
                    "Distributed trace sampling specification",
                ),
            ],
        )],
    );
    let api_calls = Arc::new(AtomicUsize::new(0));
    let http_calls = Arc::new(AtomicUsize::new(0));
    let headless_calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    let calls = Arc::clone(&api_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("api", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                api_results
            })
            .await,
        Some(SearchTierDecision::Stop)
    );

    let calls = Arc::clone(&http_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("http", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );
    let calls = Arc::clone(&headless_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("headless", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );

    assert_eq!(api_calls.load(Ordering::SeqCst), 1);
    assert_eq!(http_calls.load(Ordering::SeqCst), 0);
    assert_eq!(headless_calls.load(Ordering::SeqCst), 0);
    assert_eq!(cascade.reports().len(), 1);
}

#[tokio::test]
async fn lazy_cascade_runs_http_after_api_failure_but_stops_before_headless() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = "malaria vaccine position paper";
    let mut api_failure = SearchResults::new();
    api_failure.add_failure(
        crate::EngineFailure::new("api", "provider_quota", "quota exhausted").with_transient(false),
    );
    let http_results = Aggregator::new().aggregate_for_query(
        query,
        vec![(
            "http".to_string(),
            vec![
                result(
                    "https://health.example/malaria-vaccine",
                    "Malaria vaccine position paper",
                    "Evidence and recommendation",
                ),
                result(
                    "https://policy.example/vaccine-paper",
                    "Vaccine position paper",
                    "Malaria evidence review",
                ),
            ],
        )],
    );
    let api_calls = Arc::new(AtomicUsize::new(0));
    let http_calls = Arc::new(AtomicUsize::new(0));
    let headless_calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    let calls = Arc::clone(&api_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("api", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                api_failure
            })
            .await,
        Some(SearchTierDecision::Continue)
    );
    let calls = Arc::clone(&http_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("http", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                http_results
            })
            .await,
        Some(SearchTierDecision::Stop)
    );
    let calls = Arc::clone(&headless_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("headless", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );

    assert_eq!(api_calls.load(Ordering::SeqCst), 1);
    assert_eq!(http_calls.load(Ordering::SeqCst), 1);
    assert_eq!(headless_calls.load(Ordering::SeqCst), 0);
    assert_eq!(cascade.reports().len(), 2);
}

#[tokio::test]
async fn lazy_cascade_reaches_headless_only_after_two_insufficient_tiers() {
    let floor = SearchQualityFloor {
        min_usable_results: 3,
        min_unique_hosts: 3,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = "cross border rail capacity assessment";
    let aggregator = Aggregator::new();
    let api_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "api".to_string(),
            vec![result(
                "https://index.example/rail",
                "Rail index",
                "General transport links",
            )],
        )],
    );
    let http_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "http".to_string(),
            vec![result(
                "https://brief.example/capacity",
                "Rail capacity brief",
                "Cross border capacity summary",
            )],
        )],
    );
    let headless_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "headless".to_string(),
            vec![
                result(
                    "https://assessment.example/rail-capacity",
                    "Cross border rail capacity assessment",
                    "Methods and findings",
                ),
                result(
                    "https://evidence.example/cross-border-rail",
                    "Cross border rail evidence",
                    "Capacity assessment results",
                ),
            ],
        )],
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    for (tier, results) in [
        ("api", api_results),
        ("http", http_results),
        ("headless", headless_results),
    ] {
        let calls = Arc::clone(&calls);
        cascade
            .run_tier_if_needed(tier, || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                results
            })
            .await
            .expect("each insufficient predecessor must activate the next tier");
    }

    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(!cascade.needs_next_tier());
    assert_eq!(cascade.reports().len(), 3);
    assert_eq!(cascade.reports()[2].decision, SearchTierDecision::Stop);
}

#[test]
fn strict_generic_floor_can_require_consensus_and_mean_alignment() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 2,
        min_aligned_results: 2,
        min_consensus_results: 1,
        min_query_match: 0.2,
        min_mean_query_match: 0.5,
    };
    let insufficient = SearchQuality {
        usable_result_count: 2,
        unique_host_count: 2,
        contributing_engine_count: 2,
        consensus_result_count: 0,
        aligned_result_count: 2,
        mean_query_match: 0.75,
    };
    assert!(!floor.is_met(&insufficient));

    let sufficient = SearchQuality {
        consensus_result_count: 1,
        ..insufficient
    };
    assert!(floor.is_met(&sufficient));
}

#[test]
fn non_finite_programmatic_alignment_is_recomputed() {
    let mut item = result(
        "https://example.test/async-trait",
        "Async trait reference",
        "Language reference",
    );
    item.query_match_score = Some(f64::NAN);
    let mut results = SearchResults::new();
    results.add_result(item);

    let quality = SearchQuality::evaluate("async trait reference", &results, 0.2);

    assert!(quality.mean_query_match.is_finite());
    assert_eq!(quality.aligned_result_count, 1);
}

#[test]
fn tier_merge_deduplicates_urls_and_preserves_independent_provenance() {
    let aggregator = Aggregator::new();
    let first = aggregator.aggregate_for_query(
        "shared evidence",
        vec![(
            "api".to_string(),
            vec![result(
                "https://example.com/report?utm_source=api",
                "Shared evidence",
                "Short",
            )],
        )],
    );
    let second = aggregator.aggregate_for_query(
        "shared evidence",
        vec![(
            "headless".to_string(),
            vec![result(
                "https://www.example.com/report",
                "Shared evidence report",
                "A richer description of the shared evidence",
            )],
        )],
    );

    let mut cascade = SearchCascade::new(SearchQuery::new("shared evidence"), Default::default());
    cascade.push_tier("api", first);
    cascade.push_tier("headless", second);

    assert_eq!(cascade.results().items().len(), 1);
    let merged = &cascade.results().items()[0];
    assert_eq!(merged.engines.len(), 2);
    assert!(merged.engines.contains("api"));
    assert!(merged.engines.contains("headless"));
    assert!(merged.content.contains("richer"));
}

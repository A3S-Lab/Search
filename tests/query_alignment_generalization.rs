//! Domain-neutral query-alignment checks across scripts and request shapes.

use a3s_search::{query_match_score, Aggregator, SearchResult};

struct Scenario {
    family: &'static str,
    query: &'static str,
    relevant_title: &'static str,
    relevant_content: &'static str,
    distractor_title: &'static str,
    distractor_content: &'static str,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        family: "english-comparison",
        query: "compare municipal heat pump lifecycle costs",
        relevant_title: "Municipal heat pump lifecycle cost comparison",
        relevant_content: "Capital, maintenance, and energy cost boundaries.",
        distractor_title: "Home decoration trends",
        distractor_content: "A gallery of interior colors.",
    },
    Scenario {
        family: "chinese-current-status",
        query: "城市防洪标准最新实施日期",
        relevant_title: "城市防洪标准最新实施日期与适用范围",
        relevant_content: "标准版本、发布日期、实施日期和过渡安排。",
        distractor_title: "城市公园游览指南",
        distractor_content: "介绍开放时间和公共交通。",
    },
    Scenario {
        family: "spanish-policy",
        query: "requisitos legales almacenamiento energía industrial",
        relevant_title: "Requisitos legales para almacenamiento de energía industrial",
        relevant_content: "Obligaciones, fechas y límites de aplicación.",
        distractor_title: "Historia de la pintura moderna",
        distractor_content: "Una introducción a varias escuelas artísticas.",
    },
    Scenario {
        family: "french-evidence-review",
        query: "preuves efficacité rénovation thermique écoles",
        relevant_title: "Preuves sur l’efficacité de la rénovation thermique des écoles",
        relevant_content: "Méthode, résultats mesurés et limites de l’étude.",
        distractor_title: "Calendrier des vacances scolaires",
        distractor_content: "Dates des congés par région.",
    },
    Scenario {
        family: "german-technical-decision",
        query: "Entscheidungshilfe industrielle Wärmerückgewinnung",
        relevant_title: "Entscheidungshilfe für industrielle Wärmerückgewinnung",
        relevant_content: "Betriebsgrenzen, Wartung und Energiebedarf im Vergleich.",
        distractor_title: "Reiseführer für Bergwanderungen",
        distractor_content: "Routen und Übernachtungsmöglichkeiten.",
    },
    Scenario {
        family: "arabic-regulation",
        query: "متطلبات سلامة تخزين الهيدروجين",
        relevant_title: "متطلبات سلامة تخزين الهيدروجين ونطاق التطبيق",
        relevant_content: "المعايير والاختبارات وحدود التشغيل.",
        distractor_title: "دليل المطاعم المحلية",
        distractor_content: "قائمة بالأطباق ومواعيد العمل.",
    },
    Scenario {
        family: "hindi-program-evaluation",
        query: "ग्रामीण जल कार्यक्रम प्रभाव मूल्यांकन",
        relevant_title: "ग्रामीण जल कार्यक्रम का प्रभाव मूल्यांकन",
        relevant_content: "माप, नमूना और अध्ययन की सीमाएँ।",
        distractor_title: "स्थानीय खेल समाचार",
        distractor_content: "साप्ताहिक प्रतियोगिताओं का सारांश।",
    },
    Scenario {
        family: "japanese-risk-analysis",
        query: "港湾物流自動化リスク分析",
        relevant_title: "港湾物流自動化のリスク分析",
        relevant_content: "運用条件、障害事例、対策の適用範囲。",
        distractor_title: "季節の料理案内",
        distractor_content: "地域の食材と調理方法。",
    },
    Scenario {
        family: "korean-market-boundary",
        query: "산업용 배터리 재활용 시장 범위",
        relevant_title: "산업용 배터리 재활용 시장 범위와 산정 기준",
        relevant_content: "포함 범위, 제외 항목, 데이터 한계를 설명한다.",
        distractor_title: "주말 등산 코스",
        distractor_content: "교통과 난이도 안내.",
    },
    Scenario {
        family: "russian-causal-analysis",
        query: "причины отказов городских тепловых сетей",
        relevant_title: "Причины отказов городских тепловых сетей",
        relevant_content: "Наблюдения, причинные ограничения и методы проверки.",
        distractor_title: "Обзор театрального сезона",
        distractor_content: "Премьеры и расписание спектаклей.",
    },
    Scenario {
        family: "portuguese-scenario-analysis",
        query: "cenários expansão transporte ferroviário regional",
        relevant_title: "Cenários para expansão do transporte ferroviário regional",
        relevant_content: "Premissas, custos, demanda e incerteza.",
        distractor_title: "Guia de fotografia urbana",
        distractor_content: "Equipamentos e locais para fotografar.",
    },
    Scenario {
        family: "turkish-source-verification",
        query: "kıyı taşkını ölçüm verisi doğrulama",
        relevant_title: "Kıyı taşkını ölçüm verisinin doğrulanması",
        relevant_content: "Örnekleme, kalibrasyon ve belirsizlik sınırları.",
        distractor_title: "Şehir festivali programı",
        distractor_content: "Konser ve etkinlik saatleri.",
    },
    Scenario {
        family: "vietnamese-maintenance-status",
        query: "tình trạng bảo trì hệ thống cảnh báo lũ",
        relevant_title: "Tình trạng bảo trì hệ thống cảnh báo lũ",
        relevant_content: "Lịch bảo trì, phạm vi vận hành và khoảng trống dữ liệu.",
        distractor_title: "Hướng dẫn trồng cây cảnh",
        distractor_content: "Ánh sáng và tưới nước theo mùa.",
    },
    Scenario {
        family: "thai-comparative-assessment",
        query: "เปรียบเทียบระบบกักเก็บพลังงานชุมชน",
        relevant_title: "เปรียบเทียบระบบกักเก็บพลังงานสำหรับชุมชน",
        relevant_content: "ต้นทุน ข้อจำกัดการทำงาน และหลักฐานที่ใช้ประเมิน",
        distractor_title: "แนะนำสถานที่ท่องเที่ยว",
        distractor_content: "ข้อมูลร้านอาหารและที่พัก",
    },
    Scenario {
        family: "hebrew-operational-guidance",
        query: "הנחיות תפעול אגירת אנרגיה עירונית",
        relevant_title: "הנחיות תפעול למערכות אגירת אנרגיה עירונית",
        relevant_content: "גבולות בטיחות, תחזוקה ודרישות ניטור.",
        distractor_title: "מדריך לאירועי תרבות",
        distractor_content: "מועדים ומקומות של הופעות.",
    },
    Scenario {
        family: "greek-historical-trend",
        query: "ιστορική εξέλιξη αστικής κατανάλωσης νερού",
        relevant_title: "Ιστορική εξέλιξη της αστικής κατανάλωσης νερού",
        relevant_content: "Χρονοσειρές, αλλαγές μέτρησης και περιορισμοί.",
        distractor_title: "Οδηγός τοπικής κουζίνας",
        distractor_content: "Παραδοσιακές συνταγές και εστιατόρια.",
    },
    Scenario {
        family: "indonesian-implementation-review",
        query: "evaluasi penerapan standar bangunan rendah karbon",
        relevant_title: "Evaluasi penerapan standar bangunan rendah karbon",
        relevant_content: "Cakupan, kepatuhan, hasil, dan keterbatasan bukti.",
        distractor_title: "Katalog pameran seni",
        distractor_content: "Daftar seniman dan jadwal kunjungan.",
    },
    Scenario {
        family: "swahili-impact-assessment",
        query: "tathmini athari usafiri wa umma vijijini",
        relevant_title: "Tathmini ya athari za usafiri wa umma vijijini",
        relevant_content: "Vipimo, sampuli, matokeo na mipaka ya ushahidi.",
        distractor_title: "Mwongozo wa mapishi ya nyumbani",
        distractor_content: "Viungo na hatua za kupika.",
    },
    Scenario {
        family: "protocol-identifier",
        query: "HTTP/3 RFC 9114 connection migration requirements",
        relevant_title: "RFC 9114 HTTP/3 connection migration requirements",
        relevant_content: "Normative requirements and protocol limitations.",
        distractor_title: "Quarterly mobile connection sales",
        distractor_content: "A commercial market newsletter.",
    },
    Scenario {
        family: "version-and-date",
        query: "ISO 14068-1:2023 effective date scope",
        relevant_title: "ISO 14068-1:2023 scope and effective date",
        relevant_content: "Document version, applicability, and transition dates.",
        distractor_title: "International calendar for 2023",
        distractor_content: "Public holidays and event dates.",
    },
];

fn query_variants(query: &str) -> [String; 5] {
    [
        query.to_string(),
        format!("  {query}  "),
        query.to_uppercase(),
        format!("({query})"),
        query.split_whitespace().collect::<Vec<_>>().join("   "),
    ]
}

#[test]
fn aligned_material_ranks_above_noise_across_query_shapes() {
    let mut trials = 0usize;

    for (index, scenario) in SCENARIOS.iter().enumerate() {
        for query in query_variants(scenario.query) {
            let relevant = SearchResult::new(
                format!("https://evidence-{index}.example/report"),
                scenario.relevant_title,
                scenario.relevant_content,
            );
            let distractor = SearchResult::new(
                format!("https://noise-{index}.example/index"),
                scenario.distractor_title,
                scenario.distractor_content,
            );

            let relevant_alignment = query_match_score(&query, &relevant);
            let distractor_alignment = query_match_score(&query, &distractor);
            assert!(
                relevant_alignment > distractor_alignment,
                "{} did not distinguish aligned evidence from noise: {relevant_alignment} <= {distractor_alignment}",
                scenario.family
            );

            let ranked = Aggregator::new().aggregate_for_query(
                &query,
                vec![("opaque-provider".to_string(), vec![distractor, relevant])],
            );
            assert_eq!(
                ranked.items()[0].normalized_url(),
                format!("evidence-{index}.example/report"),
                "{} left a lower-alignment result above the material result",
                scenario.family
            );
            trials += 1;
        }
    }

    assert_eq!(trials, 100);
}

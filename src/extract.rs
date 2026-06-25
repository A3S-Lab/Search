//! Main article content extraction from raw HTML via the Readability algorithm.

use dom_smoothie::Readability;

/// Extracts the main article text from raw HTML, dropping nav, ads, and boilerplate.
///
/// Returns `None` when no meaningful article body is found (e.g. a link farm or a
/// page the algorithm cannot score). CPU-bound and synchronous — call it from
/// `spawn_blocking` when running inside an async task.
pub fn extract_main_text(html: &str, url: &str) -> Option<String> {
    // The base URL only resolves relative links; we just want the plain text, so a
    // malformed URL must not nuke extraction — fall back to no base URL if it fails.
    let mut readability = Readability::new(html, Some(url), None)
        .or_else(|_| Readability::new(html, None, None))
        .ok()?;
    let article = readability.parse().ok()?;
    // `text_content` is the cleaned plain text (vs `content`, which is cleaned HTML).
    let text = article.text_content.trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_body_and_drops_chrome() {
        // Fixture mixes English and Chinese; markers verify body kept, chrome dropped.
        let html = r#"<html><head><title>T</title></head><body>
            <nav>home about contact UNIQUE_NAV login signup</nav>
            <article>
              <h1>Main Story 正文标题</h1>
              <p>UNIQUE_BODY 这是文章的主体段落，Readability 应当判定为正文并保留。
              It contains several full sentences so the algorithm scores it as the
              main content rather than discarding it as page noise.</p>
              <p>A second substantial paragraph adds more textual weight to the
              article node, ensuring the extractor selects this block over the
              surrounding navigation, sidebar, and footer chrome on the page.</p>
              <p>A third paragraph repeats the pattern with enough words to keep the
              density high and the link ratio low, which is what the scoring heuristic
              rewards when choosing the dominant content container of a document.</p>
            </article>
            <footer>UNIQUE_FOOTER copyright 2026</footer>
        </body></html>"#;
        let text = extract_main_text(html, "https://example.com/a").expect("should extract body");
        assert!(text.contains("UNIQUE_BODY"), "kept the article body");
        assert!(!text.contains("UNIQUE_NAV"), "dropped the nav");
        assert!(!text.contains("UNIQUE_FOOTER"), "dropped the footer");
    }

    #[test]
    fn returns_none_on_empty_html() {
        assert!(extract_main_text("", "https://example.com").is_none());
    }

    #[test]
    fn extracts_chinese_article() {
        let html = r#"<html><head><title>标题</title></head><body>
            <nav>首页 关于 联系 UNIQUE_NAV 登录 注册</nav>
            <article>
              <h1>中文文章标题</h1>
              <p>UNIQUE_BODY 这是一篇中文文章的正文主体，包含足够多的句子，
              使 Readability 算法能够把它识别为页面的主要内容，而不是当成噪声丢弃。
              这一段需要足够长，因此再补充几句说明性的文字来提高文本密度。</p>
              <p>第二段同样由多句中文组成，用来增强主体节点的权重，确保抽取出来的是正文，
              而不是周围的导航栏、侧边栏或页脚等样板内容。继续补充文字以保证段落长度。</p>
              <p>第三段进一步提高正文密度并降低链接占比，这正是评分启发式在选择主要内容容器时
              所偏好的特征，从而让算法稳定地选中这个文章区块。</p>
            </article>
            <footer>UNIQUE_FOOTER 版权所有 2026</footer>
        </body></html>"#;
        let text = extract_main_text(html, "https://example.cn/a").expect("chinese body");
        assert!(text.contains("中文文章的正文主体"), "kept the chinese body");
        assert!(!text.contains("UNIQUE_FOOTER"), "dropped the footer");
    }

    #[test]
    fn malformed_html_does_not_panic() {
        // Unclosed tags and non-HTML input must be handled gracefully, never panic.
        let _ = extract_main_text("<html><body><p>oops <div><span>", "https://x.test");
        let _ = extract_main_text("not html at all, just plain text", "https://x.test");
        let _ = extract_main_text("<<>>&;#", "https://x.test");
    }

    #[test]
    fn output_has_no_surrounding_whitespace() {
        let html = "<html><body><article><p>   ALPHA body content padded with \
            enough words and sentences here to clear the readability content \
            threshold so that extraction returns a non-empty article body.   </p>\
            </article></body></html>";
        let text =
            extract_main_text(html, "https://x.test").expect("fixture should extract a body");
        assert_eq!(text, text.trim(), "no leading/trailing whitespace");
        assert!(
            text.starts_with("ALPHA"),
            "kept body, trimmed the leading padding"
        );
    }

    #[test]
    fn extracts_with_malformed_base_url() {
        // A non-URL base string must not prevent text extraction (it only resolves links).
        let html = r#"<html><body><article>
            <p>MARKER body content padded with enough words and full sentences to
            clear the readability content threshold during extraction in this test.</p>
            <p>A second paragraph adds further density so the main node is selected.</p>
            </article></body></html>"#;
        let text = extract_main_text(html, "u0").expect("extract despite bad base url");
        assert!(text.contains("MARKER"));
    }
}

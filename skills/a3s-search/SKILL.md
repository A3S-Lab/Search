---
name: a3s-search
description: Search, compare, and verify current web evidence with the a3s-search CLI and its native AnySearch and Tavily providers. Use for web research, fact checking, source discovery, current-event verification, domain-filtered search, or collecting structured answers, citations, full text, images, relevance scores, provider reports, and partial-failure evidence.
---

# A3S Search

Use the CLI first. Request JSON whenever evidence must be inspected, compared, or cited.

## Search

1. Confirm provider readiness:

   ```bash
   a3s-search engines
   ```

2. Select providers deliberately:

   - Use `anysearch` for broad discovery and AnySearch vertical routing. This
     integration follows the downloaded AnySearch Skill's MCP `tools/call`
     contract, not AnySearch's separate `/v1/search` REST schema.
     The CLI implements the Skill's `search` operation only. Use the official
     Skill's `get_sub_domains` operation before inventing a vertical
     `sub_domain`; its required parameters must be copied into ACL.
   - Use `tavily` for ranked results, direct answers, raw content, images, and usage metadata.
   - Use both for independent corroboration.
   - Add conventional engines only when they materially improve coverage.

3. Run a structured search:

   ```bash
   a3s-search "current Rust async runtime guidance" \
     --engines anysearch,tavily \
     --format json \
     --limit 10
   ```

4. Inspect `answers`, `results`, `images`, `reports`, and `errors`. Preserve URLs, provider reports, relevance scores, dates, and full text when they support the conclusion. Do not claim that a provider succeeded unless the JSON evidence shows it. Treat `auto_parameters_truncated` or `metadata_truncated` as a signal that auxiliary provider metadata was safely shortened.
   Treat `_a3s_normalization.changed = true` as evidence that invalid or
   oversized provider-controlled output was safely normalized; inspect its
   counters before relying on omitted evidence.

5. Cross-check consequential or time-sensitive claims across independent sources. Distinguish source publication dates from the current date.

## Authenticate safely

Use either provider without credentials when its documented anonymous/keyless service is sufficient:

```bash
unset ANYSEARCH_API_KEY TAVILY_API_KEY TAVILY_PROJECT
a3s-search "query" --engines anysearch,tavily --format json
```

Set environment variables for authenticated requests:

```bash
export ANYSEARCH_API_KEY="..."
export TAVILY_API_KEY="..."
export TAVILY_PROJECT="..."
```

Never print, commit, interpolate into shell history, or copy secret values into research output. Prefer `env("VARIABLE")` in ACL. `TAVILY_PROJECT` is sent only with authenticated Tavily requests.

## Configure providers with ACL

Create an ACL file when provider-specific controls are needed:

```acl
timeout {
  value = 20
}

provider "anysearch" {
  api_key = env("ANYSEARCH_API_KEY")
  max_results = 10
  domain = "code"
  sub_domain = "code.doc"
  sub_domain_params = {
    library = "tokio"
  }
}

provider "tavily" {
  api_key = env("TAVILY_API_KEY")
  project = env("TAVILY_PROJECT")
  search_depth = "advanced"
  chunks_per_source = 3
  max_results = 10
  topic = "general"
  include_answer = "advanced"
  include_raw_content = "markdown"
  include_domains = ["docs.rs", "rust-lang.org"]
  exclude_domains = ["example.com"]
  auto_parameters = true
  include_usage = true
  include_images = true
  include_image_descriptions = true
  include_favicon = true
}
```

Run:

```bash
a3s-search --config search.acl engines
a3s-search "query" --config search.acl --format json
```

Set `api_key = null` to force anonymous/keyless mode. Keep AnySearch `sub_domain` prefixed by its matching `domain`. Use `chunks_per_source` only with Tavily `search_depth = "advanced"`.
Tavily follows the official `include_usage = false` default, so enable it
explicitly when credit evidence is required.
When `auto_parameters = true`, omit `search_depth` and `topic` if Tavily should
choose them; explicit values intentionally override Tavily's automatic choices.
Treat a missing report value as unknown when Tavily does not disclose an
automatically selected depth or topic.

## Handle partial failures

Treat provider warnings and the JSON `errors` entries as evidence. Continue with usable results when one provider fails, disclose the failed provider, and avoid conclusions that depend only on missing evidence. Retry with one provider at a time to isolate authentication, quota, timeout, or configuration failures.

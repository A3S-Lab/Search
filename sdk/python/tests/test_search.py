"""Comprehensive tests for a3s-search Python SDK."""

import pytest

from a3s_search import (
    A3SSearch,
    A3SSearchError,
    EngineError,
    SearchError,
    SearchResult,
    SearchOptions,
    SearchResponse,
    EngineErrorInfo,
)


# =============================================================================
# Unit Tests — Exception Classes
# =============================================================================


class TestA3SSearchError:
    """Tests for the base exception class."""

    def test_is_exception(self):
        err = A3SSearchError("test")
        assert isinstance(err, Exception)

    def test_preserves_message(self):
        err = A3SSearchError("something went wrong")
        assert str(err) == "something went wrong"

    def test_is_catchable_as_exception(self):
        with pytest.raises(Exception):
            raise A3SSearchError("test")


class TestSearchError:
    """Tests for the SearchError exception."""

    def test_extends_base(self):
        err = SearchError("search failed")
        assert isinstance(err, A3SSearchError)
        assert isinstance(err, Exception)

    def test_preserves_message(self):
        err = SearchError("timeout exceeded")
        assert str(err) == "timeout exceeded"

    def test_catchable_as_base(self):
        with pytest.raises(A3SSearchError):
            raise SearchError("test")

    def test_distinguishable_from_engine_error(self):
        search_err = SearchError("search")
        engine_err = EngineError("engine", "msg")
        assert not isinstance(search_err, EngineError)
        assert not isinstance(engine_err, SearchError)


class TestEngineError:
    """Tests for the EngineError exception."""

    def test_extends_base(self):
        err = EngineError("DuckDuckGo", "timed out")
        assert isinstance(err, A3SSearchError)
        assert isinstance(err, Exception)

    def test_has_engine_and_message(self):
        err = EngineError("Brave", "CAPTCHA detected")
        assert err.engine == "Brave"
        assert err.message == "CAPTCHA detected"

    def test_str_format(self):
        err = EngineError("Google", "blocked")
        assert str(err) == "Google: blocked"

    def test_catchable_as_base(self):
        with pytest.raises(A3SSearchError):
            raise EngineError("engine", "msg")


# =============================================================================
# Unit Tests — Dataclasses
# =============================================================================


class TestSearchResult:
    """Tests for the SearchResult dataclass."""

    def test_required_fields(self):
        result = SearchResult(
            url="https://example.com",
            title="Example",
            content="Description",
        )
        assert result.url == "https://example.com"
        assert result.title == "Example"
        assert result.content == "Description"

    def test_default_values(self):
        result = SearchResult(url="u", title="t", content="c")
        assert result.result_type == "web"
        assert result.engines == []
        assert result.score == 0.0
        assert result.thumbnail is None
        assert result.published_date is None

    def test_all_fields(self):
        result = SearchResult(
            url="https://example.com",
            title="Example",
            content="Description",
            result_type="image",
            engines=["ddg", "brave"],
            score=2.5,
            thumbnail="https://example.com/thumb.jpg",
            published_date="2024-01-15",
        )
        assert result.result_type == "image"
        assert result.engines == ["ddg", "brave"]
        assert result.score == 2.5
        assert result.thumbnail == "https://example.com/thumb.jpg"
        assert result.published_date == "2024-01-15"

    def test_equality(self):
        a = SearchResult(url="u", title="t", content="c")
        b = SearchResult(url="u", title="t", content="c")
        assert a == b

    def test_inequality(self):
        a = SearchResult(url="u1", title="t", content="c")
        b = SearchResult(url="u2", title="t", content="c")
        assert a != b

    def test_engines_default_is_independent(self):
        """Each instance should get its own engines list."""
        a = SearchResult(url="u", title="t", content="c")
        b = SearchResult(url="u", title="t", content="c")
        a.engines.append("ddg")
        assert b.engines == []


class TestSearchOptions:
    """Tests for the SearchOptions dataclass."""

    def test_all_defaults(self):
        opts = SearchOptions()
        assert opts.engines is None
        assert opts.limit is None
        assert opts.timeout is None
        assert opts.proxy is None

    def test_all_fields(self):
        opts = SearchOptions(
            engines=["ddg", "wiki"],
            limit=10,
            timeout=15,
            proxy="http://127.0.0.1:8080",
        )
        assert opts.engines == ["ddg", "wiki"]
        assert opts.limit == 10
        assert opts.timeout == 15
        assert opts.proxy == "http://127.0.0.1:8080"

    def test_partial_fields(self):
        opts = SearchOptions(engines=["brave"], limit=5)
        assert opts.engines == ["brave"]
        assert opts.limit == 5
        assert opts.timeout is None
        assert opts.proxy is None


class TestEngineErrorInfo:
    """Tests for the EngineErrorInfo dataclass."""

    def test_fields(self):
        err = EngineErrorInfo(engine="DuckDuckGo", message="timed out")
        assert err.engine == "DuckDuckGo"
        assert err.message == "timed out"

    def test_equality(self):
        a = EngineErrorInfo(engine="e", message="m")
        b = EngineErrorInfo(engine="e", message="m")
        assert a == b


class TestSearchResponse:
    """Tests for the SearchResponse dataclass."""

    def test_required_fields(self):
        response = SearchResponse(results=[], count=0, duration_ms=42)
        assert response.results == []
        assert response.count == 0
        assert response.duration_ms == 42
        assert response.errors == []

    def test_with_results_and_errors(self):
        result = SearchResult(url="u", title="t", content="c")
        error = EngineErrorInfo(engine="brave", message="CAPTCHA")
        response = SearchResponse(
            results=[result],
            count=1,
            duration_ms=100,
            errors=[error],
        )
        assert len(response.results) == 1
        assert len(response.errors) == 1
        assert response.count == 1

    def test_errors_default_is_independent(self):
        """Each instance should get its own errors list."""
        a = SearchResponse(results=[], count=0, duration_ms=0)
        b = SearchResponse(results=[], count=0, duration_ms=0)
        a.errors.append(EngineErrorInfo(engine="e", message="m"))
        assert b.errors == []


# =============================================================================
# Unit Tests — A3SSearch Input Validation
# =============================================================================


class TestA3SSearchConstructor:
    """Tests for A3SSearch construction."""

    def test_create_instance(self):
        search = A3SSearch()
        assert search is not None

    def test_multiple_independent_instances(self):
        a = A3SSearch()
        b = A3SSearch()
        assert a is not b


class TestA3SSearchInputValidation:
    """Tests for A3SSearch input validation (no network)."""

    @pytest.mark.asyncio
    async def test_reject_empty_string(self):
        search = A3SSearch()
        with pytest.raises(SearchError):
            await search.search("")

    @pytest.mark.asyncio
    async def test_reject_whitespace_only(self):
        search = A3SSearch()
        with pytest.raises(SearchError):
            await search.search("   ")

    @pytest.mark.asyncio
    async def test_reject_tab_only(self):
        search = A3SSearch()
        with pytest.raises(SearchError):
            await search.search("\t\t")

    @pytest.mark.asyncio
    async def test_reject_newline_only(self):
        search = A3SSearch()
        with pytest.raises(SearchError):
            await search.search("\n\n")

    @pytest.mark.asyncio
    async def test_reject_mixed_whitespace(self):
        search = A3SSearch()
        with pytest.raises(SearchError):
            await search.search(" \t\n ")

    @pytest.mark.asyncio
    async def test_empty_query_error_message(self):
        search = A3SSearch()
        with pytest.raises(SearchError, match="empty"):
            await search.search("")


class TestA3SSearchEngineValidation:
    """Tests for engine shortcut validation (requires native module)."""

    @pytest.mark.asyncio
    async def test_reject_unknown_engine(self):
        search = A3SSearch()
        with pytest.raises(Exception):
            await search.search("test", engines=["nonexistent"])

    @pytest.mark.asyncio
    async def test_unknown_engine_message_contains_name(self):
        search = A3SSearch()
        with pytest.raises(Exception, match="foobar"):
            await search.search("test", engines=["foobar"])

    @pytest.mark.asyncio
    async def test_reject_empty_engines_list(self):
        search = A3SSearch()
        with pytest.raises(Exception):
            await search.search("test", engines=[])

    @pytest.mark.asyncio
    async def test_reject_all_unknown_engines(self):
        search = A3SSearch()
        with pytest.raises(Exception):
            await search.search("test", engines=["x", "y", "z"])


# =============================================================================
# Integration Tests — Real Search (requires network)
# =============================================================================


class TestA3SSearchIntegration:
    """Integration tests that perform real searches."""

    @pytest.mark.asyncio
    async def test_search_default_engines(self):
        search = A3SSearch()
        response = await search.search("rust programming language")

        assert response is not None
        assert isinstance(response.results, list)
        assert response.count >= 0
        assert response.duration_ms >= 0
        assert isinstance(response.errors, list)
        assert response.count == len(response.results)

    @pytest.mark.asyncio
    async def test_search_ddg(self):
        search = A3SSearch()
        response = await search.search("typescript", engines=["ddg"])
        assert response is not None
        assert response.count == len(response.results)

    @pytest.mark.asyncio
    async def test_search_duckduckgo_alias(self):
        search = A3SSearch()
        response = await search.search("javascript", engines=["duckduckgo"])
        assert response is not None

    @pytest.mark.asyncio
    async def test_search_wiki(self):
        search = A3SSearch()
        response = await search.search(
            "Python programming", engines=["wiki"], limit=3
        )
        assert response is not None
        assert len(response.results) <= 3

    @pytest.mark.asyncio
    async def test_search_wikipedia_alias(self):
        search = A3SSearch()
        response = await search.search("Linux", engines=["wikipedia"])
        assert response is not None

    @pytest.mark.asyncio
    async def test_search_brave(self):
        search = A3SSearch()
        response = await search.search("open source", engines=["brave"])
        assert response is not None

    @pytest.mark.asyncio
    async def test_search_sogou(self):
        search = A3SSearch()
        response = await search.search("搜索引擎", engines=["sogou"])
        assert response is not None

    @pytest.mark.asyncio
    async def test_search_360(self):
        search = A3SSearch()
        response = await search.search("人工智能", engines=["360"])
        assert response is not None

    @pytest.mark.asyncio
    async def test_search_so360_alias(self):
        search = A3SSearch()
        response = await search.search("机器学习", engines=["so360"])
        assert response is not None

    @pytest.mark.asyncio
    async def test_search_multiple_engines(self):
        search = A3SSearch()
        response = await search.search(
            "web development", engines=["ddg", "wiki", "brave"]
        )
        assert response is not None
        assert response.count == len(response.results)

    @pytest.mark.asyncio
    async def test_limit_option(self):
        search = A3SSearch()
        response = await search.search(
            "programming", engines=["ddg"], limit=2
        )
        assert len(response.results) <= 2
        assert response.count <= 2

    @pytest.mark.asyncio
    async def test_timeout_option(self):
        search = A3SSearch()
        response = await search.search(
            "test query", engines=["ddg"], timeout=20
        )
        assert response is not None

    @pytest.mark.asyncio
    async def test_result_structure(self):
        search = A3SSearch()
        response = await search.search(
            "wikipedia test", engines=["wiki"], limit=2
        )

        for result in response.results:
            assert isinstance(result.url, str)
            assert len(result.url) > 0
            assert isinstance(result.title, str)
            assert isinstance(result.content, str)
            assert isinstance(result.result_type, str)
            assert isinstance(result.engines, list)
            assert len(result.engines) > 0
            assert isinstance(result.score, float)
            assert result.score >= 0

            if result.thumbnail is not None:
                assert isinstance(result.thumbnail, str)
            if result.published_date is not None:
                assert isinstance(result.published_date, str)

    @pytest.mark.asyncio
    async def test_consistent_count(self):
        search = A3SSearch()
        response = await search.search("nodejs", engines=["ddg"])
        assert response.count == len(response.results)

    @pytest.mark.asyncio
    async def test_non_negative_duration(self):
        search = A3SSearch()
        response = await search.search("test", engines=["ddg"])
        assert response.duration_ms >= 0

    @pytest.mark.asyncio
    async def test_concurrent_searches(self):
        import asyncio

        search = A3SSearch()
        results = await asyncio.gather(
            search.search("rust", engines=["ddg"], limit=2),
            search.search("python", engines=["ddg"], limit=2),
            search.search("javascript", engines=["ddg"], limit=2),
        )
        assert len(results) == 3
        for r in results:
            assert r is not None

    @pytest.mark.asyncio
    async def test_concurrent_different_instances(self):
        import asyncio

        s1 = A3SSearch()
        s2 = A3SSearch()
        r1, r2 = await asyncio.gather(
            s1.search("rust", engines=["ddg"], limit=2),
            s2.search("python", engines=["wiki"], limit=2),
        )
        assert r1 is not None
        assert r2 is not None

"""Tests for a3s-search Python SDK."""

import pytest

from a3s_search import A3SSearch, SearchError


class TestA3SSearch:
    """Tests for the A3SSearch client."""

    def test_create_instance(self):
        """Should create an instance."""
        search = A3SSearch()
        assert search is not None

    @pytest.mark.asyncio
    async def test_reject_empty_query(self):
        """Should reject empty query."""
        search = A3SSearch()
        with pytest.raises(SearchError):
            await search.search("")
        with pytest.raises(SearchError):
            await search.search("   ")

    @pytest.mark.asyncio
    async def test_reject_unknown_engine(self):
        """Should reject unknown engine."""
        search = A3SSearch()
        with pytest.raises(Exception):
            await search.search("test", engines=["nonexistent"])

    @pytest.mark.asyncio
    async def test_search_default_engines(self):
        """Should search with default engines."""
        search = A3SSearch()
        response = await search.search("rust programming language")

        assert response is not None
        assert isinstance(response.results, list)
        assert response.count >= 0
        assert response.duration_ms >= 0
        assert isinstance(response.errors, list)

    @pytest.mark.asyncio
    async def test_search_specific_engines(self):
        """Should search with specific engines."""
        search = A3SSearch()
        response = await search.search(
            "python",
            engines=["ddg"],
            limit=3,
        )

        assert response is not None
        assert len(response.results) <= 3

    @pytest.mark.asyncio
    async def test_result_types(self):
        """Should return properly typed results."""
        search = A3SSearch()
        response = await search.search(
            "wikipedia test",
            engines=["wiki"],
            limit=2,
        )

        for result in response.results:
            assert isinstance(result.url, str)
            assert isinstance(result.title, str)
            assert isinstance(result.content, str)
            assert isinstance(result.result_type, str)
            assert isinstance(result.engines, list)
            assert isinstance(result.score, float)

    @pytest.mark.asyncio
    async def test_timeout_option(self):
        """Should respect timeout option."""
        search = A3SSearch()
        response = await search.search(
            "test query",
            engines=["ddg"],
            timeout=15,
        )

        assert response is not None

    @pytest.mark.asyncio
    async def test_limit_option(self):
        """Should respect limit option."""
        search = A3SSearch()
        response = await search.search(
            "javascript",
            engines=["ddg"],
            limit=1,
        )

        assert len(response.results) <= 1

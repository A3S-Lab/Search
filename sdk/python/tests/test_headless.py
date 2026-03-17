"""Tests for headless browser engines (Google, Baidu, BingChina).

These tests require the headless feature to be enabled and will download
Chrome for Testing on first run if not already installed.
"""

import pytest
from a3s_search import A3SSearch


class TestHeadlessEngines:
    """Integration tests for headless browser engines."""

    @pytest.mark.asyncio
    async def test_google_engine_available(self):
        """Test that Google engine can be used."""
        search = A3SSearch()
        # Just verify the engine is recognized, don't assert on results
        # (Google may serve CAPTCHA to headless browsers)
        response = await search.search("rust programming", engines=["google"], limit=3)
        assert response is not None
        assert isinstance(response.results, list)
        print(f"Google returned {response.count} results")

    @pytest.mark.asyncio
    async def test_google_alias(self):
        """Test that 'g' alias works for Google."""
        search = A3SSearch()
        response = await search.search("python", engines=["g"], limit=3)
        assert response is not None

    @pytest.mark.asyncio
    async def test_baidu_engine_available(self):
        """Test that Baidu engine can be used."""
        search = A3SSearch()
        response = await search.search("搜索引擎", engines=["baidu"], limit=3)
        assert response is not None
        assert isinstance(response.results, list)
        print(f"Baidu returned {response.count} results")

    @pytest.mark.asyncio
    async def test_bingchina_engine_available(self):
        """Test that BingChina engine can be used."""
        search = A3SSearch()
        response = await search.search("人工智能", engines=["bingchina"], limit=3)
        assert response is not None
        assert isinstance(response.results, list)
        print(f"BingChina returned {response.count} results")

    @pytest.mark.asyncio
    async def test_mixed_http_and_headless_engines(self):
        """Test using both HTTP and headless engines together."""
        search = A3SSearch()
        response = await search.search(
            "web development",
            engines=["ddg", "google", "wiki"],
            limit=5
        )
        assert response is not None
        assert isinstance(response.results, list)
        # Should have results from at least one engine
        assert response.count >= 0

    @pytest.mark.asyncio
    async def test_headless_with_proxy(self):
        """Test that proxy configuration works with headless engines."""
        search = A3SSearch()
        # This will fail if proxy is not available, but tests the code path
        try:
            response = await search.search(
                "test",
                engines=["google"],
                proxy="http://127.0.0.1:8080",
                limit=2
            )
            # If proxy is available, should get response
            assert response is not None
        except Exception as e:
            # If proxy is not available, should get connection error
            assert "proxy" in str(e).lower() or "connection" in str(e).lower()

    @pytest.mark.asyncio
    async def test_headless_response_structure(self):
        """Test that headless engines return properly structured results."""
        search = A3SSearch()
        response = await search.search("wikipedia", engines=["google"], limit=2)

        assert response is not None
        assert hasattr(response, 'results')
        assert hasattr(response, 'count')
        assert hasattr(response, 'duration_ms')
        assert hasattr(response, 'errors')
        assert hasattr(response, 'suggestions')
        assert hasattr(response, 'answers')

        # Check result structure if any results returned
        for result in response.results:
            assert isinstance(result.url, str)
            assert isinstance(result.title, str)
            assert isinstance(result.content, str)
            assert isinstance(result.engines, list)
            assert "google" in [e.lower() for e in result.engines]

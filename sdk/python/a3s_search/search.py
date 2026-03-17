"""Ergonomic Python wrapper around the native a3s-search module."""

from typing import Optional

from .errors import SearchError
from .types import EngineErrorInfo, SearchResponse, SearchResult

try:
    from a3s_search._a3s_search import PySearch, PySearchOptions
except ImportError as e:
    raise ImportError(
        "Failed to import native module 'a3s_search._a3s_search'. "
        "Did you run 'maturin develop'? "
        f"Original error: {e}"
    ) from e


class A3SSearch:
    """A3S Search client.

    Provides an ergonomic Python wrapper around the native Rust search engine.

    Example::

        from a3s_search import A3SSearch

        search = A3SSearch()
        response = await search.search("rust programming")
        for r in response.results:
            print(f"{r.title}: {r.url}")

    Example with proxy pool::

        search = A3SSearch()
        await search.set_proxy_pool([
            "http://10.0.0.1:8080",
            "http://10.0.0.2:8080",
        ])
        response = await search.search("rust programming")
    """

    def __init__(self) -> None:
        self._native = PySearch()

    async def set_proxy_pool(self, urls: list[str]) -> None:
        """Set the proxy pool URLs for IP rotation.

        Replaces any existing proxies. Automatically enables the proxy pool.

        Args:
            urls: List of proxy URLs (http/https/socks5).
        """
        await self._native.set_proxy_pool(urls)

    def set_proxy_pool_enabled(self, enabled: bool) -> None:
        """Enable or disable the proxy pool.

        When disabled, requests are made directly without a proxy.
        """
        self._native.set_proxy_pool_enabled(enabled)

    def is_proxy_pool_enabled(self) -> bool:
        """Returns whether the proxy pool is currently enabled."""
        return self._native.is_proxy_pool_enabled()

    async def proxy_pool_size(self) -> int:
        """Returns the number of proxies in the pool."""
        return await self._native.proxy_pool_size()

    async def search(
        self,
        query: str,
        *,
        engines: Optional[list[str]] = None,
        limit: Optional[int] = None,
        timeout: Optional[int] = None,
        proxy: Optional[str] = None,
        proxy_pool: Optional[list[str]] = None,
        language: Optional[str] = None,
        safesearch: Optional[str] = None,
        page: Optional[int] = None,
        time_range: Optional[str] = None,
        category: Optional[str] = None,
        engine_weights: Optional[dict[str, float]] = None,
        health_max_failures: Optional[int] = None,
        health_suspend_secs: Optional[int] = None,
        browser: Optional[str] = None,
        chrome_path: Optional[str] = None,
        lightpanda_path: Optional[str] = None,
        max_tabs: Optional[int] = None,
    ) -> SearchResponse:
        """Perform a search query.

        Args:
            query: The search query string.
            engines: Engine shortcuts to use. Defaults to ["ddg", "wiki"].
                Available: ddg, brave, bing, wiki, sogou, 360, google, baidu, bingchina.
                Note: google, baidu, bingchina require headless browser (slower but more reliable).
            limit: Maximum number of results to return.
            timeout: Per-engine timeout in seconds. Defaults to 10.
            proxy: HTTP/SOCKS5 proxy URL.
            proxy_pool: Proxy pool URLs for IP rotation.
                When provided, proxies are rotated round-robin per request.
                Takes precedence over `proxy` if both are set.
            language: Search language (e.g. "en", "zh", "ja").
            safesearch: Safe search level: "off", "moderate", or "strict".
            page: Page number for pagination (1-indexed).
            time_range: Time range filter: "day", "week", "month", or "year".
            category: Search category (e.g. "general", "images", "videos", "news").
            engine_weights: Per-engine weight multipliers (e.g. {"ddg": 1.5, "brave": 0.8}).
            health_max_failures: Maximum consecutive failures before suspending an engine.
            health_suspend_secs: Suspension duration in seconds after max failures reached.
            browser: Browser backend for headless engines: "chrome" or "lightpanda". Defaults to "lightpanda".
            chrome_path: Path to Chrome executable (only used when browser_backend is "chrome").
            lightpanda_path: Path to Lightpanda executable (only used when browser_backend is "lightpanda").
            max_tabs: Maximum concurrent browser tabs. Defaults to 4.

        Returns:
            A SearchResponse containing results and metadata.

        Raises:
            SearchError: If the search operation fails.
        """
        if not query or not query.strip():
            raise SearchError("Query cannot be empty")

        try:
            native_opts = PySearchOptions(
                engines=engines,
                limit=limit,
                timeout=timeout,
                proxy=proxy,
                proxy_pool=proxy_pool,
                language=language,
                safesearch=safesearch,
                page=page,
                time_range=time_range,
                category=category,
                engine_weights=engine_weights,
                health_max_failures=health_max_failures,
                health_suspend_secs=health_suspend_secs,
                browser=browser,
                chrome_path=chrome_path,
                lightpanda_path=lightpanda_path,
                max_tabs=max_tabs,
            )

            response = await self._native.search(query, native_opts)

            results = [
                SearchResult(
                    url=r.url,
                    title=r.title,
                    content=r.content,
                    result_type=r.result_type,
                    engines=r.engines,
                    score=r.score,
                    thumbnail=r.thumbnail,
                    published_date=r.published_date,
                )
                for r in response.results
            ]

            errors = [
                EngineErrorInfo(engine=e.engine, message=e.message)
                for e in response.errors
            ]

            return SearchResponse(
                results=results,
                count=response.count,
                duration_ms=response.duration_ms,
                errors=errors,
                suggestions=response.suggestions,
                answers=response.answers,
            )
        except SearchError:
            raise
        except Exception as e:
            raise SearchError(f"Search failed: {e}") from e

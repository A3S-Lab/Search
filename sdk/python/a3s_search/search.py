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
    """

    def __init__(self) -> None:
        self._native = PySearch()

    async def search(
        self,
        query: str,
        *,
        engines: Optional[list[str]] = None,
        limit: Optional[int] = None,
        timeout: Optional[int] = None,
        proxy: Optional[str] = None,
    ) -> SearchResponse:
        """Perform a search query.

        Args:
            query: The search query string.
            engines: Engine shortcuts to use. Defaults to ["ddg", "wiki"].
            limit: Maximum number of results to return.
            timeout: Per-engine timeout in seconds. Defaults to 10.
            proxy: HTTP/SOCKS5 proxy URL.

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
            )
        except SearchError:
            raise
        except Exception as e:
            raise SearchError(f"Search failed: {e}") from e

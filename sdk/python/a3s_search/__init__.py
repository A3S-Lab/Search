"""a3s-search: Native Python bindings for the a3s meta search engine."""

from .errors import A3SSearchError, EngineError, SearchError
from .search import A3SSearch
from .types import EngineErrorInfo, SearchOptions, SearchResponse, SearchResult

__all__ = [
    "A3SSearch",
    "A3SSearchError",
    "EngineError",
    "SearchError",
    "SearchResult",
    "SearchOptions",
    "SearchResponse",
    "EngineErrorInfo",
    "ensure_chrome",
]


async def ensure_chrome() -> str:
    """Download Chrome for Testing if not already installed.

    Returns the path to the Chrome executable.
    Can be called at import time or manually to pre-download Chrome.
    """
    from a3s_search._a3s_search import ensure_chrome as _ensure_chrome

    return await _ensure_chrome()

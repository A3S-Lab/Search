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
]

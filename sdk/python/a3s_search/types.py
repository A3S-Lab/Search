"""Type definitions for a3s-search SDK."""

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class SearchResult:
    """A single search result."""

    url: str
    """Result URL."""

    title: str
    """Result title."""

    content: str
    """Result description/snippet."""

    result_type: str = "web"
    """Type of result (e.g. "web", "image", "video")."""

    engines: list[str] = field(default_factory=list)
    """Names of engines that returned this result."""

    score: float = 0.0
    """Calculated relevance score."""

    thumbnail: Optional[str] = None
    """Thumbnail URL, if available."""

    published_date: Optional[str] = None
    """Published date, if available."""


@dataclass
class SearchOptions:
    """Options for configuring a search request."""

    engines: Optional[list[str]] = None
    """Engine shortcuts to use. Defaults to ["ddg", "wiki"]."""

    limit: Optional[int] = None
    """Maximum number of results to return."""

    timeout: Optional[int] = None
    """Per-engine timeout in seconds. Defaults to 10."""

    proxy: Optional[str] = None
    """HTTP/SOCKS5 proxy URL."""


@dataclass
class EngineErrorInfo:
    """An error from a specific search engine."""

    engine: str
    """Name of the engine that failed."""

    message: str
    """Error message."""


@dataclass
class SearchResponse:
    """Aggregated search response."""

    results: list[SearchResult]
    """The search results."""

    count: int
    """Total number of results."""

    duration_ms: int
    """Search duration in milliseconds."""

    errors: list[EngineErrorInfo] = field(default_factory=list)
    """Engine errors that occurred during search."""

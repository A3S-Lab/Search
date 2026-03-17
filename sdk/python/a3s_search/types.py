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
    """Engine shortcuts to use. Defaults to ["ddg", "wiki"].
    Available: ddg, brave, bing, wiki, sogou, 360, google, baidu, bingchina.
    Note: google, baidu, bingchina require headless browser (slower but more reliable)."""

    limit: Optional[int] = None
    """Maximum number of results to return."""

    timeout: Optional[int] = None
    """Per-engine timeout in seconds. Defaults to 10."""

    proxy: Optional[str] = None
    """HTTP/SOCKS5 proxy URL."""

    proxy_pool: Optional[list[str]] = None
    """Proxy pool URLs for IP rotation.
    When provided, proxies are rotated round-robin per request.
    Takes precedence over `proxy` if both are set."""

    language: Optional[str] = None
    """Search language (e.g. "en", "zh", "ja")."""

    safesearch: Optional[str] = None
    """Safe search level: "off", "moderate", or "strict"."""

    page: Optional[int] = None
    """Page number for pagination (1-indexed)."""

    time_range: Optional[str] = None
    """Time range filter: "day", "week", "month", or "year"."""

    category: Optional[str] = None
    """Search category (e.g. "general", "images", "videos", "news")."""

    engine_weights: Optional[dict[str, float]] = None
    """Per-engine weight multipliers (e.g. {"ddg": 1.5, "brave": 0.8})."""

    health_max_failures: Optional[int] = None
    """Maximum consecutive failures before suspending an engine."""

    health_suspend_secs: Optional[int] = None
    """Suspension duration in seconds after max failures reached."""

    browser: Optional[str] = None
    """Browser backend for headless engines: "chrome" or "lightpanda". Defaults to "lightpanda"."""

    chrome_path: Optional[str] = None
    """Path to Chrome executable (only used when browser_backend is "chrome")."""

    lightpanda_path: Optional[str] = None
    """Path to Lightpanda executable (only used when browser_backend is "lightpanda")."""

    max_tabs: Optional[int] = None
    """Maximum concurrent browser tabs. Defaults to 4."""


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

    suggestions: list[str] = field(default_factory=list)
    """Search suggestions (related queries)."""

    answers: list[str] = field(default_factory=list)
    """Instant answers (e.g. calculator results, definitions)."""

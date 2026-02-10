"""Exception classes for a3s-search SDK."""


class A3SSearchError(Exception):
    """Base exception for a3s-search SDK."""

    pass


class SearchError(A3SSearchError):
    """The search operation failed."""

    pass


class EngineError(A3SSearchError):
    """A specific engine encountered an error."""

    def __init__(self, engine: str, message: str):
        self.engine = engine
        self.message = message
        super().__init__(f"{engine}: {message}")

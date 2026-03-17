#!/usr/bin/env python3
"""Test Lightpanda backend for headless search engines.

This example demonstrates how to use Lightpanda as the headless browser
backend for Google, Baidu, and BingChina search engines.

Lightpanda is the default backend when the 'lightpanda' feature is enabled.
It's lighter and faster than Chrome, but only supports Linux and macOS.
"""

import asyncio
from a3s_search import A3SSearch


async def test_lightpanda_default():
    """Test Lightpanda with default configuration (auto-download)."""
    print("=== Test 1: Lightpanda (default, auto-download) ===")

    search = A3SSearch()

    # Lightpanda is the default backend, no need to specify
    response = await search.search(
        "Rust programming language",
        engines=["google"],
        limit=5,
        timeout=30,
    )

    print(f"Found {response.count} results in {response.duration_ms}ms")
    for i, result in enumerate(response.results[:3], 1):
        print(f"{i}. {result.title}")
        print(f"   {result.url}")

    if response.errors:
        print("\nErrors:")
        for error in response.errors:
            print(f"  - {error.engine}: {error.message}")


async def test_lightpanda_explicit():
    """Test Lightpanda with explicit backend selection."""
    print("\n=== Test 2: Lightpanda (explicit) ===")

    search = A3SSearch()

    # Explicitly specify Lightpanda backend
    response = await search.search(
        "人工智能",
        engines=["baidu"],
        limit=5,
        timeout=30,
        browser_backend="lightpanda",  # Explicit (same as default)
        max_tabs=2,  # Limit concurrent tabs
    )

    print(f"Found {response.count} results in {response.duration_ms}ms")
    for i, result in enumerate(response.results[:3], 1):
        print(f"{i}. {result.title}")
        print(f"   {result.url}")


async def test_chrome_fallback():
    """Test Chrome backend as fallback."""
    print("\n=== Test 3: Chrome (fallback) ===")

    search = A3SSearch()

    # Use Chrome instead of Lightpanda
    response = await search.search(
        "Python async",
        engines=["google"],
        limit=5,
        timeout=30,
        browser_backend="chrome",  # Use Chrome instead
    )

    print(f"Found {response.count} results in {response.duration_ms}ms")
    for i, result in enumerate(response.results[:3], 1):
        print(f"{i}. {result.title}")


async def main():
    """Run all tests."""
    await test_lightpanda_default()
    # await test_lightpanda_explicit()
    # await test_chrome_fallback()


if __name__ == "__main__":
    asyncio.run(main())

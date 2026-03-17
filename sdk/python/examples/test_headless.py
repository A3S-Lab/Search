#!/usr/bin/env python3
"""
Example script to test headless browser engines.

This script demonstrates using Google, Baidu, and BingChina engines
which require a headless browser (Chrome).

On first run, Chrome for Testing will be automatically downloaded
to ~/.a3s/chromium/ if not already installed.
"""

import asyncio
from a3s_search import A3SSearch


async def test_google():
    """Test Google search with headless browser."""
    print("\n" + "="*60)
    print("Testing Google engine (headless browser)")
    print("="*60)

    search = A3SSearch()

    try:
        response = await search.search(
            "rust programming language",
            engines=["google"],
            limit=5
        )

        print(f"\n✓ Search completed in {response.duration_ms}ms")
        print(f"✓ Found {response.count} results")

        if response.errors:
            print(f"\n⚠ Errors: {len(response.errors)}")
            for err in response.errors:
                print(f"  - {err.engine}: {err.message}")

        if response.results:
            print("\nTop results:")
            for i, result in enumerate(response.results[:3], 1):
                print(f"\n{i}. {result.title}")
                print(f"   URL: {result.url}")
                print(f"   Score: {result.score:.2f}")
                print(f"   Engines: {', '.join(result.engines)}")
        else:
            print("\n⚠ No results returned (Google may have served a CAPTCHA)")

    except Exception as e:
        print(f"\n✗ Error: {e}")
        raise


async def test_baidu():
    """Test Baidu search with headless browser."""
    print("\n" + "="*60)
    print("Testing Baidu engine (headless browser)")
    print("="*60)

    search = A3SSearch()

    try:
        response = await search.search(
            "人工智能",
            engines=["baidu"],
            limit=5
        )

        print(f"\n✓ Search completed in {response.duration_ms}ms")
        print(f"✓ Found {response.count} results")

        if response.results:
            print("\nTop results:")
            for i, result in enumerate(response.results[:3], 1):
                print(f"\n{i}. {result.title}")
                print(f"   URL: {result.url}")
                print(f"   Score: {result.score:.2f}")

    except Exception as e:
        print(f"\n✗ Error: {e}")
        raise


async def test_mixed_engines():
    """Test mixing HTTP and headless engines."""
    print("\n" + "="*60)
    print("Testing mixed engines (HTTP + headless)")
    print("="*60)

    search = A3SSearch()

    try:
        response = await search.search(
            "web development",
            engines=["ddg", "google", "wiki"],
            limit=10
        )

        print(f"\n✓ Search completed in {response.duration_ms}ms")
        print(f"✓ Found {response.count} results")

        # Count results by engine
        engine_counts = {}
        for result in response.results:
            for engine in result.engines:
                engine_counts[engine] = engine_counts.get(engine, 0) + 1

        print("\nResults by engine:")
        for engine, count in sorted(engine_counts.items()):
            print(f"  - {engine}: {count} results")

    except Exception as e:
        print(f"\n✗ Error: {e}")
        raise


async def test_new_features():
    """Test new SearchQuery features."""
    print("\n" + "="*60)
    print("Testing new SearchQuery features")
    print("="*60)

    search = A3SSearch()

    try:
        response = await search.search(
            "python tutorial",
            engines=["ddg"],
            language="en",
            safesearch="moderate",
            page=1,
            time_range="week",
            limit=5
        )

        print(f"\n✓ Search with filters completed in {response.duration_ms}ms")
        print(f"✓ Found {response.count} results")
        print(f"✓ Suggestions: {len(response.suggestions)}")
        print(f"✓ Answers: {len(response.answers)}")

        if response.suggestions:
            print("\nSearch suggestions:")
            for suggestion in response.suggestions[:5]:
                print(f"  - {suggestion}")

        if response.answers:
            print("\nInstant answers:")
            for answer in response.answers[:3]:
                print(f"  - {answer}")

    except Exception as e:
        print(f"\n✗ Error: {e}")
        raise


async def main():
    """Run all tests."""
    print("\n" + "="*60)
    print("A3S Search - Headless Engine Test Suite")
    print("="*60)
    print("\nNote: On first run, Chrome for Testing will be downloaded")
    print("to ~/.a3s/chromium/ (approximately 150-200 MB)")
    print("\nThis may take 1-5 minutes depending on your network speed...")

    try:
        # Test new features first (faster, no browser needed)
        await test_new_features()

        # Test headless engines
        await test_google()
        await test_baidu()
        await test_mixed_engines()

        print("\n" + "="*60)
        print("✓ All tests completed successfully!")
        print("="*60)

    except Exception as e:
        print("\n" + "="*60)
        print(f"✗ Test suite failed: {e}")
        print("="*60)
        raise


if __name__ == "__main__":
    asyncio.run(main())

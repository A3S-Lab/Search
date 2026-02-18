"""Post-install: download Chrome for Testing if not present.

This script is run automatically after `pip install a3s-search` to ensure
Chrome is available for headless search engines (Google, Baidu, Bing China).

Can also be run manually:
    python -m a3s_search.ensure_chrome
"""


def main():
    try:
        from a3s_search._a3s_search import ensure_chrome_sync

        path = ensure_chrome_sync()
        print(f"a3s-search: Chrome ready at {path}")
    except Exception as e:
        print(f"a3s-search: Chrome auto-download skipped ({e})")
        print("  Run `python -m a3s_search.ensure_chrome` to download later.")


if __name__ == "__main__":
    main()

/**
 * Post-install: download Chrome for Testing if not present.
 *
 * This script runs automatically after `npm install @a3s-lab/search` to ensure
 * Chrome is available for headless search engines (Google, Baidu, Bing China).
 */

try {
  const { ensureChrome } = require("../index");
  ensureChrome()
    .then((path) => {
      console.log(`a3s-search: Chrome ready at ${path}`);
    })
    .catch((err) => {
      console.log(`a3s-search: Chrome auto-download skipped (${err.message})`);
      console.log(
        '  Run: node -e "require(\'@a3s-lab/search\').ensureChrome()"'
      );
    });
} catch {
  // Native module not yet built (e.g. during CI prebuild), skip silently.
}

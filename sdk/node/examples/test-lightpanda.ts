/**
 * Test Lightpanda backend for headless search engines.
 *
 * This example demonstrates how to use Lightpanda as the headless browser
 * backend for Google, Baidu, and BingChina search engines.
 *
 * Lightpanda is the default backend when the 'lightpanda' feature is enabled.
 * It's lighter and faster than Chrome, but only supports Linux and macOS.
 */

import { Search } from '../index.js';

async function testLightpandaDefault() {
  console.log('=== Test 1: Lightpanda (default, auto-download) ===');

  const search = new Search();

  // Lightpanda is the default backend, no need to specify
  const response = await search.search('Rust programming language', {
    engines: ['google'],
    limit: 5,
    timeout: 30,
  });

  console.log(`Found ${response.count} results in ${response.durationMs}ms`);
  response.results.slice(0, 3).forEach((result, i) => {
    console.log(`${i + 1}. ${result.title}`);
    console.log(`   ${result.url}`);
  });

  if (response.errors.length > 0) {
    console.log('\nErrors:');
    response.errors.forEach(error => {
      console.log(`  - ${error.engine}: ${error.message}`);
    });
  }
}

async function testLightpandaExplicit() {
  console.log('\n=== Test 2: Lightpanda (explicit) ===');

  const search = new Search();

  // Explicitly specify Lightpanda backend
  const response = await search.search('人工智能', {
    engines: ['baidu'],
    limit: 5,
    timeout: 30,
    browserBackend: 'lightpanda', // Explicit (same as default)
    maxTabs: 2, // Limit concurrent tabs
  });

  console.log(`Found ${response.count} results in ${response.durationMs}ms`);
  response.results.slice(0, 3).forEach((result, i) => {
    console.log(`${i + 1}. ${result.title}`);
    console.log(`   ${result.url}`);
  });
}

async function testLightpandaCustomPath() {
  console.log('\n=== Test 3: Lightpanda (custom path) ===');

  const search = new Search();

  try {
    // Use custom Lightpanda binary (if you built it yourself)
    const response = await search.search('machine learning', {
      engines: ['bingchina'],
      limit: 5,
      timeout: 30,
      browserBackend: 'lightpanda',
      lightpandaPath: '/usr/local/bin/lightpanda', // Custom path
    });

    console.log(`Found ${response.count} results in ${response.durationMs}ms`);
  } catch (error) {
    console.log(`Note: Custom path test failed (expected if binary not at path): ${error.message}`);
  }
}

async function testChromeFallback() {
  console.log('\n=== Test 4: Chrome (fallback) ===');

  const search = new Search();

  // Use Chrome instead of Lightpanda
  const response = await search.search('Python async', {
    engines: ['google'],
    limit: 5,
    timeout: 30,
    browserBackend: 'chrome', // Use Chrome instead
  });

  console.log(`Found ${response.count} results in ${response.durationMs}ms`);
  response.results.slice(0, 3).forEach((result, i) => {
    console.log(`${i + 1}. ${result.title}`);
  });
}

async function main() {
  await testLightpandaDefault();
  await testLightpandaExplicit();
  await testLightpandaCustomPath();
  await testChromeFallback();
}

main().catch(console.error);

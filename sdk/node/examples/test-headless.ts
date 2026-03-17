/**
 * Example script to test headless browser engines.
 *
 * This script demonstrates using Google, Baidu, and BingChina engines
 * which require a headless browser (Chrome).
 *
 * On first run, Chrome for Testing will be automatically downloaded
 * to ~/.a3s/chromium/ if not already installed.
 */

import { A3SSearch } from '../lib/index.js';

async function testGoogle() {
  console.log('\n' + '='.repeat(60));
  console.log('Testing Google engine (headless browser)');
  console.log('='.repeat(60));

  const search = new A3SSearch();

  try {
    const response = await search.search('rust programming language', {
      engines: ['google'],
      limit: 5,
    });

    console.log(`\n✓ Search completed in ${response.durationMs}ms`);
    console.log(`✓ Found ${response.count} results`);

    if (response.errors.length > 0) {
      console.log(`\n⚠ Errors: ${response.errors.length}`);
      for (const err of response.errors) {
        console.log(`  - ${err.engine}: ${err.message}`);
      }
    }

    if (response.results.length > 0) {
      console.log('\nTop results:');
      for (let i = 0; i < Math.min(3, response.results.length); i++) {
        const result = response.results[i];
        console.log(`\n${i + 1}. ${result.title}`);
        console.log(`   URL: ${result.url}`);
        console.log(`   Score: ${result.score.toFixed(2)}`);
        console.log(`   Engines: ${result.engines.join(', ')}`);
      }
    } else {
      console.log('\n⚠ No results returned (Google may have served a CAPTCHA)');
    }
  } catch (error) {
    console.log(`\n✗ Error: ${error.message}`);
    throw error;
  }
}

async function testBaidu() {
  console.log('\n' + '='.repeat(60));
  console.log('Testing Baidu engine (headless browser)');
  console.log('='.repeat(60));

  const search = new A3SSearch();

  try {
    const response = await search.search('人工智能', {
      engines: ['baidu'],
      limit: 5,
    });

    console.log(`\n✓ Search completed in ${response.durationMs}ms`);
    console.log(`✓ Found ${response.count} results`);

    if (response.results.length > 0) {
      console.log('\nTop results:');
      for (let i = 0; i < Math.min(3, response.results.length); i++) {
        const result = response.results[i];
        console.log(`\n${i + 1}. ${result.title}`);
        console.log(`   URL: ${result.url}`);
        console.log(`   Score: ${result.score.toFixed(2)}`);
      }
    }
  } catch (error) {
    console.log(`\n✗ Error: ${error.message}`);
    throw error;
  }
}

async function testMixedEngines() {
  console.log('\n' + '='.repeat(60));
  console.log('Testing mixed engines (HTTP + headless)');
  console.log('='.repeat(60));

  const search = new A3SSearch();

  try {
    const response = await search.search('web development', {
      engines: ['ddg', 'google', 'wiki'],
      limit: 10,
    });

    console.log(`\n✓ Search completed in ${response.durationMs}ms`);
    console.log(`✓ Found ${response.count} results`);

    // Count results by engine
    const engineCounts: Record<string, number> = {};
    for (const result of response.results) {
      for (const engine of result.engines) {
        engineCounts[engine] = (engineCounts[engine] || 0) + 1;
      }
    }

    console.log('\nResults by engine:');
    for (const [engine, count] of Object.entries(engineCounts).sort()) {
      console.log(`  - ${engine}: ${count} results`);
    }
  } catch (error) {
    console.log(`\n✗ Error: ${error.message}`);
    throw error;
  }
}

async function testNewFeatures() {
  console.log('\n' + '='.repeat(60));
  console.log('Testing new SearchQuery features');
  console.log('='.repeat(60));

  const search = new A3SSearch();

  try {
    const response = await search.search('python tutorial', {
      engines: ['ddg'],
      language: 'en',
      safesearch: 'moderate',
      page: 1,
      timeRange: 'week',
      limit: 5,
    });

    console.log(`\n✓ Search with filters completed in ${response.durationMs}ms`);
    console.log(`✓ Found ${response.count} results`);
    console.log(`✓ Suggestions: ${response.suggestions.length}`);
    console.log(`✓ Answers: ${response.answers.length}`);

    if (response.suggestions.length > 0) {
      console.log('\nSearch suggestions:');
      for (const suggestion of response.suggestions.slice(0, 5)) {
        console.log(`  - ${suggestion}`);
      }
    }

    if (response.answers.length > 0) {
      console.log('\nInstant answers:');
      for (const answer of response.answers.slice(0, 3)) {
        console.log(`  - ${answer}`);
      }
    }
  } catch (error) {
    console.log(`\n✗ Error: ${error.message}`);
    throw error;
  }
}

async function main() {
  console.log('\n' + '='.repeat(60));
  console.log('A3S Search - Headless Engine Test Suite');
  console.log('='.repeat(60));
  console.log('\nNote: On first run, Chrome for Testing will be downloaded');
  console.log('to ~/.a3s/chromium/ (approximately 150-200 MB)');
  console.log('\nThis may take 1-5 minutes depending on your network speed...');

  try {
    // Test new features first (faster, no browser needed)
    await testNewFeatures();

    // Test headless engines
    await testGoogle();
    await testBaidu();
    await testMixedEngines();

    console.log('\n' + '='.repeat(60));
    console.log('✓ All tests completed successfully!');
    console.log('='.repeat(60));
  } catch (error) {
    console.log('\n' + '='.repeat(60));
    console.log(`✗ Test suite failed: ${error}`);
    console.log('='.repeat(60));
    throw error;
  }
}

main().catch(console.error);

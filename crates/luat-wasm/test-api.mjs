/**
 * Test file for the Luat WASM JavaScript API
 * Run with: node test-api.mjs
 */

import { initLuat, addTemplate, render, renderWithError, clearTemplates, version, LuatEngine } from './luat.js';

async function runTests() {
  console.log('Testing Luat WASM JavaScript API\n');
  let passed = 0;
  let failed = 0;

  function test(name, fn) {
    try {
      fn();
      console.log(`✓ ${name}`);
      passed++;
    } catch (e) {
      console.log(`✗ ${name}`);
      console.log(`  Error: ${e.message}`);
      failed++;
    }
  }

  // Initialize
  console.log('Initializing Luat...');
  await initLuat();
  console.log('Initialized!\n');

  // Test version
  test('version returns string', () => {
    const v = version();
    if (typeof v !== 'string' || v.length === 0) {
      throw new Error(`Expected version string, got: ${v}`);
    }
    console.log(`  Version: ${v}`);
  });

  // Test simple render
  test('render simple template', () => {
    clearTemplates();
    addTemplate('test.luat', '<div>Hello World</div>');
    const result = render('test.luat', {});
    if (!result.includes('Hello World')) {
      throw new Error(`Unexpected result: ${result}`);
    }
  });

  // Test props
  test('render with props', () => {
    clearTemplates();
    addTemplate('test.luat', '<h1>Hello, {props.name}!</h1>');
    const result = render('test.luat', { name: 'World' });
    if (!result.includes('Hello, World!')) {
      throw new Error(`Unexpected result: ${result}`);
    }
  });

  // Test if block
  test('if block', () => {
    clearTemplates();
    addTemplate('test.luat', '{#if props.show}<p>Visible</p>{/if}');
    const result = render('test.luat', { show: true });
    if (!result.includes('Visible')) {
      throw new Error(`Expected 'Visible', got: ${result}`);
    }
  });

  // Test each block
  test('each block', () => {
    clearTemplates();
    addTemplate('test.luat', '<ul>{#each props.items as item}<li>{item}</li>{/each}</ul>');
    const result = render('test.luat', { items: ['A', 'B', 'C'] });
    if (!result.includes('A') || !result.includes('B') || !result.includes('C')) {
      throw new Error(`Missing items in: ${result}`);
    }
  });

  // Test component import
  test('component import', () => {
    clearTemplates();
    addTemplate('Button.luat', '<button class="btn">{@render props.children?.()}</button>');
    addTemplate('main.luat', `<script>
local Button = require("Button")
</script>
<Button>Click me</Button>`);
    const result = render('main.luat', {});
    if (!result.includes('btn') || !result.includes('Click me')) {
      throw new Error(`Missing button content: ${result}`);
    }
  });

  // Test renderWithError - success case
  test('renderWithError success', () => {
    clearTemplates();
    addTemplate('test.luat', '<p>Success</p>');
    const result = renderWithError('test.luat', {});
    if (!result.success) {
      throw new Error(`Expected success, got error: ${result.error}`);
    }
    if (!result.html.includes('Success')) {
      throw new Error(`Unexpected HTML: ${result.html}`);
    }
  });

  // Test renderWithError - error case
  test('renderWithError handles errors', () => {
    clearTemplates();
    // Try to render non-existent template
    const result = renderWithError('nonexistent.luat', {});
    if (result.success) {
      throw new Error('Expected error for non-existent template');
    }
    if (!result.error) {
      throw new Error('Expected error message');
    }
  });

  // Test HTML escaping
  test('HTML escaping', () => {
    clearTemplates();
    addTemplate('test.luat', '<p>{props.text}</p>');
    const result = render('test.luat', { text: '<script>alert("xss")</script>' });
    if (result.includes('<script>')) {
      throw new Error('XSS not escaped!');
    }
  });

  // Test LuatEngine class
  test('LuatEngine class', async () => {
    const engine = new LuatEngine();
    await engine.init();
    engine
      .clearTemplates()
      .addTemplate('test.luat', '<span>{props.x}</span>');
    const result = engine.render('test.luat', { x: 42 });
    if (!result.includes('42')) {
      throw new Error(`Expected 42, got: ${result}`);
    }
  });

  // Summary
  console.log(`\n${passed} passed, ${failed} failed`);
  process.exit(failed > 0 ? 1 : 0);
}

runTests().catch(e => {
  console.error('Test runner error:', e);
  process.exit(1);
});

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const vendor = readFileSync(join(__dirname, '../crates/devoption-ui/assets/vendor/highlight.min.js'), 'utf8');
const enhance = readFileSync(join(__dirname, '../crates/devoption-ui/assets/code-browser.js'), 'utf8');

test('the bundled highlighter preserves escaped source and highlights supported languages', () => {
  const context = {};
  runInNewContext(vendor, context);
  for (const [language, text] of [['rust', 'fn main() { println!("<script>evil()</script>"); }'], ['javascript', 'const a = "hello";'], ['ini', '[package]\nname = "example"']]) {
    const result = context.hljs.highlight(text, { language }).value;
    assert.match(result, /class="hljs-/);
    assert.doesNotMatch(result, /<script>/);
    assert.equal(result.replace(/<[^>]+>/g, '').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&amp;/g, '&'), text);
  }
  assert.doesNotMatch(vendor, /<\/script/i);
});

test('unknown extensions stay literal and Markdown fences use only known languages', () => {
  const nodes = [
    { dataset: { sourcePath: 'src/main.rs' } },
    { dataset: { sourcePath: 'data.unknown' } },
    { dataset: {}, classList: ['language-javascript'] },
    { dataset: {}, classList: ['language-unknown'] },
  ];
  const highlighted = [];
  runInNewContext(enhance, {
    document: { querySelectorAll: () => nodes },
    hljs: { getLanguage: name => ['rust', 'javascript'].includes(name), highlightElement: code => highlighted.push(code) },
  });
  assert.deepEqual(highlighted, [nodes[0], nodes[2]]);
});

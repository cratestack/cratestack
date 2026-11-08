// The TextMate grammar highlights the multi-file keywords (`part`, `part of`,
// `import`; cratestack#923). Stands in for a screenshot of a highlighted
// split schema: each grammar pattern is run as a plain JS RegExp against the
// lines `examples/react-vite-swr` actually uses.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const grammar = JSON.parse(
  fs.readFileSync(path.join(__dirname, '..', 'syntaxes', 'cstack.tmLanguage.json'), 'utf8'),
);

// Oniguruma-only constructs a JS RegExp would reject or misread.
const ONIGURUMA_ONLY = /\(\?<[a-z]|\\[hHGzZ]|\(\?[imx-]+[:)]|\[\[:|\\p\{|\(\?~/;

function compile(rule) {
  assert.ok(!ONIGURUMA_ONLY.test(rule.match), `Oniguruma-only syntax in ${rule.match}`);
  return new RegExp(rule.match, 'g');
}

const keywordRules = grammar.repository.keywords.patterns;
const keywordPatterns = keywordRules.map(compile);
const [partOf] = keywordPatterns;
const bareKeywords = keywordPatterns[keywordPatterns.length - 1];
const string = grammar.repository.strings.patterns[0];

function matches(re, line) {
  re.lastIndex = 0;
  return [...line.matchAll(re)].map((m) => m[0]);
}

test('every grammar pattern is plain JS regex syntax', () => {
  for (const group of Object.values(grammar.repository)) {
    for (const rule of group.patterns) {
      if (rule.match) compile(rule);
    }
  }
});

test('`part of` is one keyword match, ahead of the bare `part`', () => {
  assert.equal(keywordRules[0].name, 'keyword.control.cstack');
  assert.deepEqual(matches(partOf, 'part of "../schema.cstack"'), ['part of']);
});

test('`part` and `import` lines highlight their keyword', () => {
  assert.deepEqual(matches(bareKeywords, 'part "x.cstack"'), ['part']);
  assert.deepEqual(matches(bareKeywords, 'import "x.cstack"'), ['import']);
  assert.deepEqual(matches(bareKeywords, 'part "schema/board.cstack"'), ['part']);
});

test('the quoted path is a string', () => {
  assert.equal(string.name, 'string.quoted.double.cstack');
  assert.equal(string.begin, '"');
  assert.equal(string.end, '"');
  const path = 'part of "../schema.cstack"'.slice('part of '.length);
  assert.match(path, new RegExp(`^${string.begin}[^"]*${string.end}$`));
});

test('existing keywords still match', () => {
  assert.deepEqual(matches(bareKeywords, 'datasource db {'), ['datasource']);
  assert.deepEqual(matches(bareKeywords, 'model Board {'), ['model']);
  assert.deepEqual(matches(bareKeywords, 'mutation procedure clearDoneTasks(args: A): B'), [
    'mutation',
    'procedure',
  ]);
  assert.deepEqual(matches(bareKeywords, 'auth Caller {'), ['auth']);
});

'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const { Checker, dictionaryCatalog } = require('../index.js');

test('word lists expose deterministic checking and suggestions', () => {
  const checker = new Checker('ferrolex\nFerris');

  assert.equal(checker.check('ferrolex'), true);
  assert.equal(checker.check('ferolex'), false);
  assert.deepEqual(checker.suggest('ferolex'), ['ferrolex']);
});

test('caller-owned Hunspell files retain recognition and ranking signals', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ferrolex-node-'));
  const affPath = path.join(directory, 'test.aff');
  const dicPath = path.join(directory, 'test.dic');
  fs.writeFileSync(
    affPath,
    'SET UTF-8\nREP 1\nREP recieve receive\nSFX S Y 1\nSFX S 0 s .\n',
  );
  fs.writeFileSync(dicPath, '2\nreceive/S\nferrolex\n');

  try {
    const checker = Checker.fromHunspell(affPath, dicPath);
    assert.equal(checker.check('receives'), true);
    assert.equal(checker.suggest('recieve')[0], 'receive');
  } finally {
    fs.rmSync(directory, { recursive: true });
  }
});

test('the managed catalog exposes pinned source and license metadata', () => {
  const english = dictionaryCatalog().find(({ locale }) => locale === 'en_US');

  assert.ok(english);
  assert.match(english.revision, /^[0-9a-f]{40}$/);
  assert.notEqual(english.license, '');
  assert.match(english.licenseNoticeUrl, /^https:\/\//);
});

test(
  'a pre-populated managed cache is verified and imported off the event loop',
  { skip: !process.env.FERROLEX_NODE_MANAGED_CACHE },
  async () => {
    const checker = await Checker.install(
      'en_US',
      process.env.FERROLEX_NODE_MANAGED_CACHE,
    );

    assert.equal(checker.check('colors'), true);
    assert.equal(checker.check('ferrolexcompatnotaword'), false);
  },
);

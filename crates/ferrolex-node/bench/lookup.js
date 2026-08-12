'use strict';

const native = require(process.argv[2]);
const words = Array.from({ length: 4096 }, (_, index) => 'word' + index);
words.push('ferrolex');
const queries = Array.from({ length: 12000 }, (_, index) =>
  index % 3 === 0 ? 'word' + (index % 4096) : 'missing' + index,
);

const checker = new native.Checker(words.join('\n'));
const baseline = new Set(words);

function measure(check) {
  let recognized = 0;
  const start = process.hrtime.bigint();
  for (const query of queries) {
    if (check(query)) {
      recognized += 1;
    }
  }
  return { elapsedNs: Number(process.hrtime.bigint() - start), recognized };
}

const nativeResult = measure((query) => checker.check(query));
const baselineResult = measure((query) => baseline.has(query));
if (nativeResult.recognized !== baselineResult.recognized) {
  throw new Error('binding and Set baseline disagree on recognized queries');
}
if (!checker.suggest('ferolex').includes('ferrolex')) {
  throw new Error('binding did not expose the expected suggestion');
}

console.log(JSON.stringify({
  binding: 'node-napi',
  queries: queries.length,
  nativeNs: nativeResult.elapsedNs,
  setBaselineNs: baselineResult.elapsedNs,
  recognized: nativeResult.recognized,
}));

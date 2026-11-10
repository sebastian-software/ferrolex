#!/usr/bin/env node
// Exercise the pinned action's Release Please engine without writing to GitHub.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { createRequire } = require('node:module');
const engine = createRequire(path.resolve(process.argv[2], 'package.json'));
const { Manifest } = engine('release-please');
assert.equal(engine('release-please/package.json').version, '17.6.0');
const root = path.resolve(__dirname, '..');
const version = JSON.parse(fs.readFileSync(path.join(root, '.release-please-manifest.json')))['.'];
const files = new Map();
const commits = [{ sha: 'feature', message: 'feat: exercise coordinated release', files: ['src/lib.rs'] }];
let existing;
let creates = 0;
let updates = 0;
let candidateFiles;
const scm = {
  repository: { owner: 'sebastian-software', repo: 'ferrolex', defaultBranch: 'main' },
  async getFileContentsOnBranch(file) {
    const parsedContent = files.get(file) ?? fs.readFileSync(path.join(root, file), 'utf8');
    return { parsedContent, content: Buffer.from(parsedContent).toString('base64'), sha: 'fixture' };
  },
  async getFileJson(file) { return JSON.parse((await this.getFileContentsOnBranch(file)).parsedContent); },
  async *releaseIterator() { yield { tagName: `ferrolex-v${version}`, sha: 'released', notes: '' }; },
  async *mergeCommitIterator() { yield* commits; yield { sha: 'released', message: 'chore: release', files: [] }; },
  async *pullRequestIterator(_branch, state) { if (state === 'OPEN' && existing) yield existing; },
  async createPullRequest(pr, _branch, _message, changes) {
    creates++;
    await apply(changes);
    existing = { ...pr, number: 1 };
    return existing;
  },
  async updatePullRequest(number, pr) {
    assert.equal(number, 1);
    assert.equal(pr.headRefName, existing.headBranchName);
    updates++;
    await apply(pr.updates);
    existing = { ...existing, body: pr.body.toString(), title: pr.title.toString() };
    return existing;
  },
};
async function apply(changes) {
  candidateFiles = new Map();
  for (const change of changes) {
    let before = candidateFiles.get(change.path);
    if (before === undefined) {
      try { before = (await scm.getFileContentsOnBranch(change.path)).parsedContent; }
      catch (error) { if (!change.createIfMissing) throw error; }
    }
    candidateFiles.set(change.path, change.updater.updateContent(before));
  }
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'ferrolex-release-candidate-'));
  try {
    fs.cpSync(root, fixture, { recursive: true, filter: (source) => !['.git', 'target', 'node_modules'].includes(path.basename(source)) });
    for (const [file, content] of candidateFiles) {
      fs.mkdirSync(path.dirname(path.join(fixture, file)), { recursive: true });
      fs.writeFileSync(path.join(fixture, file), content);
    }
    execFileSync(process.env.PYTHON || 'python3', ['scripts/check-release-version-contract.py'], { cwd: fixture, stdio: 'inherit' });
    execFileSync(process.env.PYTHON || 'python3', ['scripts/publish-crates.py', '--check'], { cwd: fixture, stdio: 'inherit' });
    assert.deepEqual(Object.keys(JSON.parse(candidateFiles.get('.release-please-manifest.json'))), ['.']);
  } finally { fs.rmSync(fixture, { recursive: true, force: true }); }
}
(async () => {
  await (await Manifest.fromManifest(scm, 'main')).createPullRequests();
  assert.equal(creates, 1, 'preparation must write exactly one release branch');
  assert.equal(updates, 0);
  commits.unshift({ sha: 'refresh', message: 'fix: refresh the pending candidate', files: ['src/lib.rs'] });
  await (await Manifest.fromManifest(scm, 'main')).createPullRequests();
  assert.equal(creates, 1);
  assert.equal(updates, 1, 'normal refresh must update the same branch exactly once');
  await (await Manifest.fromManifest(scm, 'main')).createPullRequests();
  assert.equal(updates, 1, 'unchanged refresh must be a no-op');
  console.log('Release preparation and normal refresh preserve one coordinated candidate.');
})().catch(error => { console.error(error); process.exitCode = 1; });

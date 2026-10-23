'use strict';
// Integration tests for the git service + diff/graph helpers, run against real temporary repos.
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const { api: g, parseStatus } = require('../src/git');
const { parseDiff, buildPatch, canPartial } = require('../renderer/diff-core');
const { computeGraph } = require('../renderer/graph');

const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'ds-test-'));
const sh = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8' });
const write = (repo, rel, text) => {
  fs.mkdirSync(path.dirname(path.join(repo, rel)), { recursive: true });
  fs.writeFileSync(path.join(repo, rel), text);
};
const read = (repo, rel) => fs.readFileSync(path.join(repo, rel), 'utf8');

const tests = [];
const test = (name, fn) => tests.push({ name, fn });

async function newRepo(name) {
  const dir = path.join(tmpRoot, name);
  const repo = await g.init(dir);
  sh(repo, 'config', 'user.name', 'Tester');
  sh(repo, 'config', 'user.email', 'tester@example.com');
  sh(repo, 'config', 'core.autocrlf', 'false');
  sh(repo, 'config', 'commit.gpgsign', 'false');
  return repo;
}

test('unborn repo: status, log, refs, unstage', async () => {
  const repo = await newRepo('unborn');
  assert.deepStrictEqual(await g.log(repo, {}), []);
  write(repo, 'a b.txt', 'hola\n');
  write(repo, 'dir/ñandú.txt', 'x\n');
  let st = await g.status(repo);
  assert.deepStrictEqual(st.unstaged.map((f) => f.path), ['a b.txt', 'dir/ñandú.txt']);
  assert.strictEqual(st.unstaged[0].status, '?');
  await g.stage(repo, ['a b.txt', 'dir/ñandú.txt']);
  st = await g.status(repo);
  assert.strictEqual(st.staged.length, 2);
  await g.unstage(repo, ['a b.txt']);
  st = await g.status(repo);
  assert.deepStrictEqual(st.staged.map((f) => f.path), ['dir/ñandú.txt']);
  const r = await g.refs(repo);
  assert.strictEqual(r.head, null);
  await g.unstageAll(repo);
  st = await g.status(repo);
  assert.strictEqual(st.staged.length, 0);
});

test('commit, log, details, branches, tags, special file names', async () => {
  const repo = await newRepo('basic');
  write(repo, 'readme.md', 'uno\ndos\ntres\n');
  write(repo, '[weird] name.txt', 'w\n');
  await g.stageAll(repo);
  const h1 = await g.commit(repo, 'Primer commit\n\nCuerpo del mensaje');
  assert.match(h1, /^[0-9a-f]{40}$/);
  let st = await g.status(repo);
  assert.strictEqual(st.staged.length + st.unstaged.length, 0);
  const d = await g.commitDetails(repo, h1);
  assert.strictEqual(d.message, 'Primer commit\n\nCuerpo del mensaje');
  assert.deepStrictEqual(d.files.map((f) => f.path).sort(), ['[weird] name.txt', 'readme.md']);
  const fd = await g.commitFileDiff(repo, d.base, h1, d.files.find((f) => f.path === 'readme.md'));
  assert.match(fd.text, /\+uno/);

  // literal pathspecs: the glob-looking name must be staged alone
  write(repo, '[weird] name.txt', 'w2\n');
  write(repo, 'w name.txt', 'should not be staged\n');
  await g.stage(repo, ['[weird] name.txt']);
  st = await g.status(repo);
  assert.deepStrictEqual(st.staged.map((f) => f.path), ['[weird] name.txt']);
  await g.commit(repo, 'segundo');
  await g.discard(repo, [], []);
  fs.unlinkSync(path.join(repo, 'w name.txt'));

  await g.createBranch(repo, 'feature/x', null, true);
  let refs = await g.refs(repo);
  assert.strictEqual(refs.currentBranch, 'feature/x');
  await assert.rejects(g.createBranch(repo, 'bad name', null, false));
  await assert.rejects(g.createBranch(repo, '-x', null, false));
  await g.createTag(repo, 'v1.0', null, 'Versión 1');
  await g.createTag(repo, 'light', h1, '');
  refs = await g.refs(repo);
  assert.deepStrictEqual(refs.tags.map((t) => t.name).sort(), ['light', 'v1.0']);
  assert.strictEqual(refs.tags.find((t) => t.name === 'light').hash, h1);
  assert.strictEqual(refs.tags.find((t) => t.name === 'v1.0').hash, refs.head);
  await g.checkout(repo, 'master').catch(() => g.checkout(repo, 'main'));
  await g.renameBranch(repo, 'feature/x', 'feature/y');
  await g.deleteBranch(repo, 'feature/y', false);
  await g.deleteTag(repo, 'light');
  refs = await g.refs(repo);
  assert.strictEqual(refs.local.length, 1);
  const log = await g.log(repo, { max: 10 });
  assert.strictEqual(log.length, 2);
  assert.strictEqual(log[1].hash, h1);
});

test('partial staging: single hunk and selected lines (LF and CRLF)', async () => {
  for (const eol of ['\n', '\r\n']) {
    const repo = await newRepo('partial' + (eol === '\n' ? 'lf' : 'crlf'));
    const lines = Array.from({ length: 30 }, (_, i) => `line ${i + 1}`);
    write(repo, 'f.txt', lines.join(eol) + eol);
    await g.stageAll(repo);
    await g.commit(repo, 'base');
    const changed = lines.slice();
    changed[1] = 'CHANGED 2';
    changed.splice(25, 0, 'ADDED A', 'ADDED B');
    write(repo, 'f.txt', changed.join(eol) + eol);

    let diff = parseDiff((await g.workingDiff(repo, { path: 'f.txt' }, false)).text);
    assert.strictEqual(diff.length, 1);
    assert.strictEqual(diff[0].hunks.length, 2);
    assert.ok(canPartial(diff[0]));

    // stage only the second hunk
    await g.applyPatch(repo, buildPatch(diff[0], diff[0].hunks[1], null, false), { cached: true });
    let staged = parseDiff((await g.workingDiff(repo, { path: 'f.txt' }, true)).text);
    assert.strictEqual(staged[0].hunks.length, 1);
    assert.ok(staged[0].hunks[0].lines.some((l) => l.startsWith('+ADDED A')));

    // unstage only the line "ADDED B"
    const h = staged[0].hunks[0];
    const idx = h.lines.findIndex((l) => l.startsWith('+ADDED B'));
    await g.applyPatch(repo, buildPatch(staged[0], h, new Set([idx]), true), { cached: true, reverse: true });
    staged = parseDiff((await g.workingDiff(repo, { path: 'f.txt' }, true)).text);
    const stagedAdds = staged[0].hunks[0].lines.filter((l) => l[0] === '+');
    assert.deepStrictEqual(stagedAdds.map((l) => l.replace(/\r$/, '')), ['+ADDED A']);

    // stage only the deletion line of hunk 1 (keep "CHANGED 2" unstaged)
    diff = parseDiff((await g.workingDiff(repo, { path: 'f.txt' }, false)).text);
    const h1 = diff[0].hunks[0];
    const del = h1.lines.findIndex((l) => l.startsWith('-line 2'));
    await g.applyPatch(repo, buildPatch(diff[0], h1, new Set([del]), false), { cached: true });
    const index = sh(repo, 'show', ':f.txt');
    assert.ok(!index.includes('line 2' + eol));
    assert.ok(!index.includes('CHANGED 2'));
    assert.ok(index.includes('ADDED A'));

    // discard the remaining working tree change of hunk "ADDED B" only
    diff = parseDiff((await g.workingDiff(repo, { path: 'f.txt' }, false)).text);
    const hb = diff[0].hunks.find((x) => x.lines.some((l) => l.startsWith('+ADDED B')));
    const ib = hb.lines.findIndex((l) => l.startsWith('+ADDED B'));
    await g.applyPatch(repo, buildPatch(diff[0], hb, new Set([ib]), true), { reverse: true });
    const wt = read(repo, 'f.txt');
    assert.ok(!wt.includes('ADDED B'));
    assert.ok(wt.includes('CHANGED 2'));
    assert.ok(wt.includes('ADDED A'));
    assert.ok(eol === '\n' || wt.split('\n').slice(0, -1).every((l) => l.endsWith('\r')), 'CRLF preserved');
  }
});

test('renames show in status and diff', async () => {
  const repo = await newRepo('rename');
  write(repo, 'old.txt', 'contenido estable\n'.repeat(20));
  await g.stageAll(repo);
  await g.commit(repo, 'base');
  fs.renameSync(path.join(repo, 'old.txt'), path.join(repo, 'new.txt'));
  await g.stageAll(repo);
  const st = await g.status(repo);
  assert.deepStrictEqual(st.staged, [{ path: 'new.txt', status: 'R', orig: 'old.txt' }]);
  const d = parseDiff((await g.workingDiff(repo, st.staged[0], true)).text);
  assert.strictEqual(d[0].renamedFrom, 'old.txt');
  await g.unstage(repo, ['new.txt', 'old.txt']);
  const st2 = await g.status(repo);
  assert.strictEqual(st2.staged.length, 0);
});

test('merge conflicts are reported, resolved and continued', async () => {
  const repo = await newRepo('conflict');
  write(repo, 'c.txt', 'base\n');
  await g.stageAll(repo);
  await g.commit(repo, 'base');
  const main = (await g.refs(repo)).currentBranch;
  await g.createBranch(repo, 'other', null, true);
  write(repo, 'c.txt', 'other\n');
  await g.stageAll(repo);
  await g.commit(repo, 'other change');
  await g.checkout(repo, main);
  write(repo, 'c.txt', 'mine\n');
  await g.stageAll(repo);
  await g.commit(repo, 'main change');
  const res = await g.merge(repo, 'other');
  assert.strictEqual(res.conflicts, true);
  let st = await g.status(repo);
  assert.strictEqual(st.state, 'merge');
  assert.deepStrictEqual(st.conflicted.map((f) => f.path), ['c.txt']);
  assert.match(st.mergeMsg, /Merge branch 'other'/);
  await g.resolveConflict(repo, 'c.txt', 'theirs');
  assert.strictEqual(read(repo, 'c.txt'), 'other\n');
  const cont = await g.continueOperation(repo, 'merge');
  assert.strictEqual(cont.conflicts, false);
  st = await g.status(repo);
  assert.strictEqual(st.state, null);
  const log = await g.log(repo, {});
  assert.strictEqual(log[0].parents.length, 2);
  const graph = computeGraph(log);
  assert.strictEqual(graph.rows.length, log.length);
  assert.ok(graph.maxLanes >= 2);
  // merge that fails for a non-conflict reason must throw
  await assert.rejects(g.merge(repo, 'does-not-exist'));
  // abort path
  await g.checkout(repo, 'other');
  write(repo, 'c.txt', 'other 2\n');
  await g.stageAll(repo);
  await g.commit(repo, 'other 2');
  await g.checkout(repo, main);
  write(repo, 'c.txt', 'mine 2\n');
  await g.stageAll(repo);
  await g.commit(repo, 'mine 2');
  assert.strictEqual((await g.merge(repo, 'other')).conflicts, true);
  await g.abortOperation(repo, 'merge');
  st = await g.status(repo);
  assert.strictEqual(st.state, null);
  assert.strictEqual(read(repo, 'c.txt'), 'mine 2\n');
});

test('stash save / apply / pop / drop, cherry-pick, revert, reset', async () => {
  const repo = await newRepo('stash');
  write(repo, 's.txt', '1\n');
  await g.stageAll(repo);
  const base = await g.commit(repo, 'base');
  write(repo, 's.txt', '2\n');
  write(repo, 'untracked.txt', 'u\n');
  await g.stashSave(repo, { message: 'mi stash', untracked: true });
  let refs = await g.refs(repo);
  assert.strictEqual(refs.stashes.length, 1);
  assert.match(refs.stashes[0].message, /mi stash/);
  assert.strictEqual(read(repo, 's.txt'), '1\n');
  assert.ok(!fs.existsSync(path.join(repo, 'untracked.txt')));
  await g.stashApply(repo, 'stash@{0}', false);
  assert.strictEqual(read(repo, 's.txt'), '2\n');
  await g.discardAll(repo, true);
  assert.strictEqual(read(repo, 's.txt'), '1\n');
  assert.ok(!fs.existsSync(path.join(repo, 'untracked.txt')));
  await g.stashApply(repo, 'stash@{0}', true);
  refs = await g.refs(repo);
  assert.strictEqual(refs.stashes.length, 0);
  await g.discardAll(repo, true);
  write(repo, 's.txt', '3\n');
  await g.stashSave(repo, {});
  await g.stashDrop(repo, 'stash@{0}');
  assert.strictEqual((await g.refs(repo)).stashes.length, 0);

  const main = (await g.refs(repo)).currentBranch;
  await g.createBranch(repo, 'pick', null, true);
  write(repo, 'p.txt', 'pick\n');
  await g.stageAll(repo);
  const pickHash = await g.commit(repo, 'to pick');
  await g.checkout(repo, main);
  assert.strictEqual((await g.cherryPick(repo, pickHash, false)).conflicts, false);
  assert.strictEqual(read(repo, 'p.txt'), 'pick\n');
  const head = (await g.refs(repo)).head;
  assert.strictEqual((await g.revert(repo, head, false)).conflicts, false);
  assert.ok(!fs.existsSync(path.join(repo, 'p.txt')));
  await g.reset(repo, base, 'hard');
  assert.strictEqual((await g.refs(repo)).head, base);
  await assert.rejects(g.reset(repo, base, 'evil'));
});

test('clone, remotes, fetch, push, pull, upstream tracking', async () => {
  const bare = path.join(tmpRoot, 'origin.git');
  sh(tmpRoot, 'init', '--bare', bare);
  const a = await newRepo('clone-a');
  write(a, 'f.txt', 'a\n');
  await g.stageAll(a);
  await g.commit(a, 'a1');
  await g.addRemote(a, 'origin', bare);
  const branch = (await g.refs(a)).currentBranch;
  await g.push(a, { remote: 'origin', branch, setUpstream: true }, 'op1');
  let refs = await g.refs(a);
  assert.strictEqual(refs.local[0].upstream, `origin/${branch}`);
  assert.strictEqual(refs.local[0].upstreamRemote, 'origin');
  assert.strictEqual(refs.local[0].upstreamBranch, branch);

  const b = await g.clone(bare, path.join(tmpRoot, 'clone-b'), 'op2');
  sh(b, 'config', 'user.name', 'B');
  sh(b, 'config', 'user.email', 'b@example.com');
  assert.strictEqual(read(b, 'f.txt').replace(/\r/g, ''), 'a\n');
  await assert.rejects(g.clone(bare, path.join(tmpRoot, 'clone-b')), /no está vacía/);

  write(b, 'f.txt', 'b\n');
  await g.stageAll(b);
  await g.commit(b, 'b1');
  await g.push(b, { remote: 'origin', branch, remoteBranch: branch }, null);

  await g.fetch(a, {}, null);
  refs = await g.refs(a);
  assert.strictEqual(refs.local[0].behind, 1);
  const pr = await g.pull(a, { remote: 'origin', branch, rebase: false }, null);
  assert.strictEqual(pr.conflicts, false);
  assert.strictEqual(read(a, 'f.txt'), 'b\n');

  await g.createBranch(b, 'feat', null, false);
  await g.push(b, { remote: 'origin', branch: 'feat', setUpstream: true }, null);
  await g.fetch(a, { remote: 'origin' }, null);
  refs = await g.refs(a);
  assert.ok(refs.remote.some((r) => r.name === 'origin/feat' && r.remote === 'origin' && r.branch === 'feat'));
  await g.checkoutRemote(a, 'origin/feat', 'feat');
  refs = await g.refs(a);
  assert.strictEqual(refs.currentBranch, 'feat');
  assert.strictEqual(refs.local.find((l) => l.name === 'feat').upstream, 'origin/feat');
  await g.checkout(a, branch);
  await g.deleteRemoteBranch(a, 'origin', 'feat', null);
  await g.createTag(a, 'v2', null, '');
  await g.pushTag(a, 'origin', 'v2', null);
  assert.match(sh(bare, 'tag'), /v2/);
  await g.deleteRemoteTag(a, 'origin', 'v2', null);
  assert.doesNotMatch(sh(bare, 'tag'), /v2/);
  const info = await g.remotesInfo(a);
  assert.deepStrictEqual(info.map((r) => r.name), ['origin']);
  await g.removeRemote(a, 'origin');
  assert.strictEqual((await g.refs(a)).remotes.length, 0);
});

test('identity, gitignore, readWorkingFile, resolveRepo', async () => {
  const repo = await newRepo('misc');
  await g.setIdentity(repo, { name: 'Local Name', email: 'local@example.com', global: false });
  const id = await g.identity(repo);
  assert.strictEqual(id.name, 'Local Name');
  write(repo, 'build/out.log', 'x');
  write(repo, '#hash.txt', 'x');
  await g.ignorePaths(repo, ['build/out.log'], true);
  await g.ignorePaths(repo, ['#hash.txt'], false);
  const st = await g.status(repo);
  assert.deepStrictEqual(st.unstaged.map((f) => f.path), ['.gitignore']);
  write(repo, 'bin.dat', Buffer.from([0, 1, 2, 3]));
  assert.strictEqual(g.readWorkingFile(repo, 'bin.dat').binary, true);
  assert.strictEqual(g.readWorkingFile(repo, '.gitignore').binary, false);
  assert.throws(() => g.readWorkingFile(repo, '../outside.txt'), /fuera del repositorio/);
  fs.mkdirSync(path.join(repo, 'sub', 'deep'), { recursive: true });
  assert.strictEqual(await g.resolveRepo(path.join(repo, 'sub', 'deep')), path.resolve(repo));
  await assert.rejects(g.resolveRepo(tmpRoot), /no es un repositorio/);
});

test('status parser handles every record type', () => {
  const raw = [
    '# branch.oid abc',
    '# branch.head main',
    '# branch.upstream origin/main',
    '# branch.ab +2 -3',
    '1 .M N... 100644 100644 100644 aaa aaa file with spaces.txt',
    '1 A. N... 000000 100644 100644 000 bbb added.txt',
    '2 R. N... 100644 100644 100644 ccc ccc R100 new name.txt',
    'old name.txt',
    'u UU N... 100644 100644 100644 100644 a b c conflict.txt',
    '? untracked file.txt',
    '! ignored.txt',
    '',
  ].join('\0');
  const st = parseStatus(raw);
  assert.strictEqual(st.branch, 'main');
  assert.strictEqual(st.ahead, 2);
  assert.strictEqual(st.behind, 3);
  assert.deepStrictEqual(st.staged, [
    { path: 'added.txt', status: 'A', orig: null },
    { path: 'new name.txt', status: 'R', orig: 'old name.txt' },
  ]);
  assert.deepStrictEqual(st.unstaged.map((f) => [f.path, f.status]), [['file with spaces.txt', 'M'], ['untracked file.txt', '?']]);
  assert.deepStrictEqual(st.conflicted, [{ path: 'conflict.txt', status: 'UU' }]);
});

test('graph: lanes for branches and merges', () => {
  // D merges B and C; B and C both come from A.
  const commits = [
    { hash: 'D', parents: ['B', 'C'] },
    { hash: 'C', parents: ['A'] },
    { hash: 'B', parents: ['A'] },
    { hash: 'A', parents: [] },
  ];
  const { rows, maxLanes } = computeGraph(commits);
  assert.strictEqual(maxLanes, 2);
  assert.strictEqual(rows[0].col, 0);
  assert.deepStrictEqual(rows[0].parentLanes, [0, 1]);
  assert.strictEqual(rows[1].col, 1);
  assert.deepStrictEqual(rows[1].parentLanes, [1]);
  assert.strictEqual(rows[2].col, 0);
  assert.deepStrictEqual(rows[2].parentLanes, [0]);
  // both lanes converge into A, which stays on the first lane
  assert.strictEqual(rows[3].col, 0);
  assert.deepStrictEqual(rows[3].before, ['A', 'A']);
});

(async () => {
  let failed = 0;
  console.log(execFileSync('git', ['--version'], { encoding: 'utf8' }).trim());
  for (const t of tests) {
    try {
      await t.fn();
      console.log(`  ok   ${t.name}`);
    } catch (e) {
      failed++;
      console.log(`  FAIL ${t.name}\n${e.stack}`);
    }
  }
  fs.rmSync(tmpRoot, { recursive: true, force: true });
  console.log(failed ? `\n${failed} test(s) failed` : `\nAll ${tests.length} tests passed`);
  process.exit(failed ? 1 : 0);
})();

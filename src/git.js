'use strict';
// Git service: every call spawns git directly (no shell) with an argument array,
// so paths, branch names and messages are never interpreted by a shell.
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const BASE_ARGS = [
  '--no-optional-locks',
  '-c', 'core.quotepath=false',
  '-c', 'color.ui=never',
  '-c', 'diff.noprefix=false',
  '-c', 'diff.mnemonicPrefix=false',
  '-c', 'log.showSignature=false',
  '-c', 'core.pager=cat',
];

const DIFF_ARGS = ['--no-color', '--no-ext-diff', '--no-textconv'];
const MAX_DIFF_BYTES = 8 * 1024 * 1024;
const LITERAL = { GIT_LITERAL_PATHSPECS: '1' };

let progressSink = null;
const running = new Map();
const gitDirCache = new Map();
const emptyTreeCache = new Map();

function setProgressSink(fn) {
  progressSink = fn;
}

function run(cwd, args, { input = null, opId = null, env = null, maxBuffer = 0 } = {}) {
  return new Promise((resolve, reject) => {
    if (!cwd || !fs.existsSync(cwd)) {
      reject(new Error(`La carpeta no existe: ${cwd}`));
      return;
    }
    let child;
    try {
      child = spawn('git', [...BASE_ARGS, ...args], {
        cwd,
        windowsHide: true,
        env: {
          ...process.env,
          GIT_TERMINAL_PROMPT: '0',
          GIT_EDITOR: 'true',
          GIT_SEQUENCE_EDITOR: 'true',
          GIT_MERGE_AUTOEDIT: 'no',
          ...(env || {}),
        },
      });
    } catch (e) {
      reject(e);
      return;
    }
    if (opId) running.set(opId, child);
    const out = [];
    const err = [];
    let outLen = 0;
    let truncated = false;
    let settled = false;
    child.stdout.on('data', (d) => {
      if (maxBuffer && outLen >= maxBuffer) {
        truncated = true;
        return;
      }
      out.push(d);
      outLen += d.length;
    });
    child.stderr.on('data', (d) => {
      err.push(d);
      if (opId && progressSink) {
        for (const line of d.toString('utf8').split(/[\r\n]+/)) {
          const t = line.trim();
          if (t) progressSink(opId, t);
        }
      }
    });
    child.stdin.on('error', () => {});
    child.on('error', (e) => {
      if (settled) return;
      settled = true;
      if (opId) running.delete(opId);
      reject(e.code === 'ENOENT'
        ? new Error('No se encontró Git. Instálalo (https://git-scm.com) y asegúrate de que está en el PATH.')
        : e);
    });
    child.on('close', (code, signal) => {
      if (settled) return;
      settled = true;
      if (opId) running.delete(opId);
      let stdout = Buffer.concat(out);
      if (maxBuffer && stdout.length > maxBuffer) {
        stdout = stdout.subarray(0, maxBuffer);
        truncated = true;
      }
      resolve({
        code,
        signal,
        stdout,
        stderr: Buffer.concat(err).toString('utf8'),
        truncated,
        cancelled: !!child.cancelled,
      });
    });
    if (input != null) child.stdin.end(input);
    else child.stdin.end();
  });
}

const PROGRESS_LINE = /^(remote: )?(Enumerating|Counting|Compressing|Writing|Receiving|Resolving|Total|Delta compression|Unpacking|Updating files|Checking out files|Filtering content)/;

function cleanOutput(text) {
  return String(text || '')
    .split(/[\r\n]+/)
    .map((l) => l.trimEnd())
    .filter((l) => l.trim() && !PROGRESS_LINE.test(l))
    .join('\n')
    .trim();
}

function failure(r, args) {
  if (r.cancelled) return new Error('Operación cancelada.');
  const msg = cleanOutput(r.stderr) || cleanOutput(r.stdout.toString('utf8'));
  return new Error(msg || `git ${args[0]} terminó con código ${r.code}`);
}

async function git(cwd, args, opts = {}) {
  const r = await run(cwd, args, opts);
  if (r.cancelled || (r.code !== 0 && !opts.allowFail)) throw failure(r, args);
  return r;
}

async function out(cwd, args, opts) {
  return (await git(cwd, args, opts)).stdout.toString('utf8');
}

function cancel(opId) {
  const child = running.get(opId);
  if (!child) return false;
  child.cancelled = true;
  if (process.platform === 'win32') {
    spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], { windowsHide: true }).on('error', () => {});
  } else {
    child.kill('SIGTERM');
  }
  return true;
}

function safeJoin(repo, rel) {
  const root = path.resolve(repo);
  const full = path.resolve(root, rel || '.');
  if (full !== root && !full.startsWith(root + path.sep)) throw new Error('Ruta fuera del repositorio.');
  return full;
}

function splitN(str, n) {
  const parts = [];
  let idx = 0;
  for (let i = 0; i < n; i++) {
    const sp = str.indexOf(' ', idx);
    if (sp === -1) break;
    parts.push(str.slice(idx, sp));
    idx = sp + 1;
  }
  parts.push(str.slice(idx));
  return parts;
}

const byPath = (a, b) => a.path.localeCompare(b.path);

// ---------------------------------------------------------------- queries

async function version() {
  return (await out(os.homedir(), ['--version'])).trim();
}

async function resolveRepo(dir) {
  if (!dir || !fs.existsSync(dir)) throw new Error('La carpeta no existe.');
  if (!fs.statSync(dir).isDirectory()) dir = path.dirname(dir);
  const r = await run(dir, ['rev-parse', '--show-toplevel']);
  if (r.code !== 0) throw new Error('La carpeta seleccionada no es un repositorio Git (o es un repositorio "bare").');
  return path.resolve(r.stdout.toString('utf8').trim());
}

async function gitDir(repo) {
  let dir = gitDirCache.get(repo);
  if (!dir) {
    dir = path.resolve((await out(repo, ['rev-parse', '--absolute-git-dir'])).trim());
    gitDirCache.set(repo, dir);
  }
  return dir;
}

async function hasHead(repo) {
  return (await run(repo, ['rev-parse', '-q', '--verify', 'HEAD'])).code === 0;
}

function parseStatus(raw) {
  const res = { oid: null, branch: null, upstream: null, ahead: 0, behind: 0, staged: [], unstaged: [], conflicted: [] };
  const toks = raw.split('\0');
  for (let i = 0; i < toks.length; i++) {
    const t = toks[i];
    if (!t) continue;
    if (t.startsWith('# ')) {
      const [key, ...rest] = t.slice(2).split(' ');
      const val = rest.join(' ');
      if (key === 'branch.oid') res.oid = val === '(initial)' ? null : val;
      else if (key === 'branch.head') res.branch = val === '(detached)' ? null : val;
      else if (key === 'branch.upstream') res.upstream = val;
      else if (key === 'branch.ab') {
        const m = /\+(\d+) -(\d+)/.exec(val);
        if (m) {
          res.ahead = +m[1];
          res.behind = +m[2];
        }
      }
      continue;
    }
    const type = t[0];
    if (type === '1' || type === '2') {
      const parts = splitN(t, type === '1' ? 8 : 9);
      const xy = parts[1];
      const file = parts[parts.length - 1];
      const orig = type === '2' ? toks[++i] : null;
      const [x, y] = xy;
      if (x !== '.') res.staged.push({ path: file, status: x, orig: x === 'R' || x === 'C' ? orig : null });
      if (y !== '.') res.unstaged.push({ path: file, status: y, orig: y === 'R' || y === 'C' ? orig : null });
    } else if (type === 'u') {
      const parts = splitN(t, 10);
      res.conflicted.push({ path: parts[10], status: parts[1] });
    } else if (type === '?') {
      res.unstaged.push({ path: t.slice(2), status: '?', orig: null });
    }
  }
  res.staged.sort(byPath);
  res.unstaged.sort(byPath);
  res.conflicted.sort(byPath);
  return res;
}

async function status(repo) {
  const [r, dir] = await Promise.all([
    git(repo, ['status', '--porcelain=v2', '-z', '--branch', '--untracked-files=all']),
    gitDir(repo),
  ]);
  const res = parseStatus(r.stdout.toString('utf8'));
  const ex = (p) => fs.existsSync(path.join(dir, p));
  if (ex('rebase-merge') || ex('rebase-apply')) res.state = 'rebase';
  else if (ex('MERGE_HEAD')) res.state = 'merge';
  else if (ex('CHERRY_PICK_HEAD')) res.state = 'cherry-pick';
  else if (ex('REVERT_HEAD')) res.state = 'revert';
  else res.state = null;
  res.mergeMsg = '';
  if (res.state && res.state !== 'rebase' && ex('MERGE_MSG')) {
    try {
      res.mergeMsg = fs.readFileSync(path.join(dir, 'MERGE_MSG'), 'utf8')
        .split(/\r?\n/).filter((l) => !l.startsWith('#')).join('\n').trim();
    } catch { /* optional */ }
  }
  return res;
}

function parseTrack(track) {
  const ahead = /ahead (\d+)/.exec(track || '');
  const behind = /behind (\d+)/.exec(track || '');
  return { ahead: ahead ? +ahead[1] : 0, behind: behind ? +behind[1] : 0, gone: track === 'gone' };
}

async function refs(repo) {
  const fmt = ['%(refname)', '%(objectname)', '%(*objectname)', '%(upstream:short)', '%(upstream:track,nobracket)',
    '%(HEAD)', '%(upstream:remotename)', '%(upstream:remoteref)'].join('%00');
  const [remotesOut, refsOut, stashR, headR, symR] = await Promise.all([
    out(repo, ['remote']),
    out(repo, ['for-each-ref', `--format=${fmt}`, 'refs/heads', 'refs/remotes', 'refs/tags']),
    run(repo, ['stash', 'list', '--format=%gd%x00%H%x00%gs']),
    run(repo, ['rev-parse', '-q', '--verify', 'HEAD']),
    run(repo, ['symbolic-ref', '-q', 'HEAD']),
  ]);
  const remotes = remotesOut.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  const byLength = [...remotes].sort((a, b) => b.length - a.length);
  const local = [];
  const remote = [];
  const tags = [];
  for (const line of refsOut.split('\n')) {
    if (!line) continue;
    const [ref, oid, peeled, upShort, track, head, upRemote, upRef] = line.split('\0');
    if (ref.startsWith('refs/heads/')) {
      local.push({
        name: ref.slice(11),
        hash: oid,
        upstream: upShort || null,
        upstreamRemote: upRemote || null,
        upstreamBranch: upRef ? upRef.replace(/^refs\/heads\//, '') : null,
        current: head === '*',
        ...parseTrack(track),
      });
    } else if (ref.startsWith('refs/remotes/')) {
      const rest = ref.slice(13);
      if (rest === 'HEAD' || rest.endsWith('/HEAD')) continue;
      let rn = byLength.find((r) => rest.startsWith(r + '/'));
      if (!rn) rn = rest.split('/')[0];
      remote.push({ remote: rn, branch: rest.slice(rn.length + 1), name: rest, hash: oid });
    } else if (ref.startsWith('refs/tags/')) {
      tags.push({ name: ref.slice(10), hash: peeled || oid });
    }
  }
  tags.sort((a, b) => b.name.localeCompare(a.name, undefined, { numeric: true }));
  const stashOut = stashR.code === 0 ? stashR.stdout.toString('utf8') : '';
  const stashes = stashOut.split('\n').filter(Boolean).map((l) => {
    const [ref, hash, message] = l.split('\0');
    return { ref, hash, message: message || '' };
  });
  const head = headR.code === 0 ? headR.stdout.toString('utf8').trim() : null;
  const currentBranch = symR.code === 0 ? symR.stdout.toString('utf8').trim().replace(/^refs\/heads\//, '') : null;
  return {
    remotes,
    local,
    remote,
    tags,
    stashes,
    head,
    currentBranch,
    sig: [refsOut, stashOut, head, currentBranch].join('\n|'),
  };
}

async function log(repo, { max = 3000, all = true } = {}) {
  const head = await hasHead(repo);
  const count = Math.max(1, Math.min(100000, Math.floor(Number(max) || 3000)));
  const args = ['log', '--date-order', `--max-count=${count}`, '--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s%x1e'];
  if (all) {
    const anyRef = (await out(repo, ['for-each-ref', '--count=1', '--format=%(refname)', 'refs/heads', 'refs/remotes', 'refs/tags'])).trim();
    if (!anyRef && !head) return [];
    args.push('--branches', '--remotes', '--tags');
    if (head) args.push('HEAD');
  } else {
    if (!head) return [];
    args.push('HEAD');
  }
  args.push('--');
  const text = await out(repo, args);
  const commits = [];
  for (let rec of text.split('\x1e')) {
    rec = rec.replace(/^\r?\n/, '');
    if (!rec) continue;
    const [hash, parents, author, email, time, subject] = rec.split('\x1f');
    commits.push({
      hash,
      parents: parents ? parents.split(' ').filter(Boolean) : [],
      author,
      email,
      time: Number(time),
      subject: subject || '',
    });
  }
  return commits;
}

async function emptyTree(repo) {
  let h = emptyTreeCache.get(repo);
  if (!h) {
    h = (await out(repo, ['hash-object', '-t', 'tree', '-w', '--stdin'], { input: '' })).trim();
    emptyTreeCache.set(repo, h);
  }
  return h;
}

function parseNameStatus(text) {
  const t = text.split('\0');
  const files = [];
  for (let i = 0; i < t.length;) {
    const s = t[i];
    if (!s) {
      i++;
      continue;
    }
    const code = s[0];
    if (code === 'R' || code === 'C') {
      files.push({ status: code, orig: t[i + 1], path: t[i + 2] });
      i += 3;
    } else {
      files.push({ status: code, orig: null, path: t[i + 1] });
      i += 2;
    }
  }
  return files;
}

async function commitDetails(repo, hash) {
  const meta = await out(repo, ['show', '-s', '--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ce%x00%ct%x00%B', hash, '--']);
  const p = meta.split('\0');
  const parents = (p[1] || '').trim().split(' ').filter(Boolean);
  const base = parents[0] || (await emptyTree(repo));
  const ns = await out(repo, ['diff', ...DIFF_ARGS, '-z', '-M', '--name-status', base, p[0], '--']);
  return {
    hash: p[0],
    parents,
    author: p[2],
    email: p[3],
    time: Number(p[4]),
    committer: p[5],
    committerEmail: p[6],
    commitTime: Number(p[7]),
    message: p.slice(8).join('\0').replace(/\s+$/, ''),
    base,
    files: parseNameStatus(ns),
  };
}

function diffResult(r) {
  const text = r.stdout.toString('utf8');
  return { text, truncated: r.truncated, lossy: text.includes('\uFFFD') };
}

async function commitFileDiff(repo, base, hash, file) {
  const paths = [file.path];
  if (file.orig) paths.push(file.orig);
  const r = await git(repo, ['diff', ...DIFF_ARGS, '-M', base, hash, '--', ...paths], { maxBuffer: MAX_DIFF_BYTES, env: LITERAL });
  return diffResult(r);
}

async function workingDiff(repo, file, staged) {
  const args = ['diff', ...DIFF_ARGS, '-M'];
  if (staged) args.push('--cached');
  args.push('--', file.path);
  if (file.orig) args.push(file.orig);
  const r = await git(repo, args, { maxBuffer: MAX_DIFF_BYTES, env: LITERAL });
  return diffResult(r);
}

function readWorkingFile(repo, rel, max = 2 * 1024 * 1024) {
  const full = safeJoin(repo, rel);
  const st = fs.statSync(full);
  if (st.isDirectory()) return { directory: true, size: 0 };
  const len = Math.min(st.size, max);
  const buf = Buffer.alloc(len);
  const fd = fs.openSync(full, 'r');
  try {
    fs.readSync(fd, buf, 0, len, 0);
  } finally {
    fs.closeSync(fd);
  }
  const binary = buf.subarray(0, 8000).includes(0);
  return { binary, size: st.size, truncated: st.size > max, text: binary ? '' : buf.toString('utf8') };
}

async function identity(repo) {
  const get = async (args) => {
    const r = await run(repo, ['config', ...args]);
    return r.code === 0 ? r.stdout.toString('utf8').trim() : '';
  };
  const [name, email, globalName, globalEmail] = await Promise.all([
    get(['--get', 'user.name']),
    get(['--get', 'user.email']),
    get(['--global', '--get', 'user.name']),
    get(['--global', '--get', 'user.email']),
  ]);
  return { name, email, globalName, globalEmail };
}

async function remotesInfo(repo) {
  const text = await out(repo, ['remote', '-v']);
  const map = new Map();
  for (const line of text.split(/\r?\n/)) {
    const m = /^(\S+)\t(.+?) \((fetch|push)\)$/.exec(line);
    if (m && (m[3] === 'fetch' || !map.has(m[1]))) map.set(m[1], m[2]);
  }
  return [...map].map(([name, url]) => ({ name, url }));
}

// ---------------------------------------------------------------- mutations

async function init(dir) {
  fs.mkdirSync(dir, { recursive: true });
  await git(dir, ['init']);
  return resolveRepo(dir);
}

async function clone(url, dest, opId) {
  if (!url || !dest) throw new Error('Indica la URL y la carpeta de destino.');
  const target = path.resolve(dest);
  if (fs.existsSync(target) && fs.readdirSync(target).length) {
    throw new Error('La carpeta de destino ya existe y no está vacía.');
  }
  const parent = path.dirname(target);
  fs.mkdirSync(parent, { recursive: true });
  await git(parent, ['clone', '--progress', '--', url, target], { opId });
  return resolveRepo(target);
}

const pathspec = (paths) => ({ input: paths.join('\0'), env: LITERAL });
const FROM_STDIN = ['--pathspec-from-file=-', '--pathspec-file-nul'];

async function stage(repo, paths) {
  if (!paths || !paths.length) return;
  await git(repo, ['add', '-A', ...FROM_STDIN], pathspec(paths));
}

async function stageAll(repo) {
  await git(repo, ['add', '-A']);
}

async function unstage(repo, paths) {
  if (!paths || !paths.length) return;
  if (await hasHead(repo)) await git(repo, ['reset', '-q', 'HEAD', ...FROM_STDIN], pathspec(paths));
  else await git(repo, ['rm', '--cached', '-r', '-q', '--ignore-unmatch', ...FROM_STDIN], pathspec(paths));
}

async function unstageAll(repo) {
  if (await hasHead(repo)) await git(repo, ['reset', '-q']);
  else await git(repo, ['rm', '--cached', '-r', '-q', '--ignore-unmatch', '.']);
}

async function discard(repo, tracked, untracked) {
  if (tracked && tracked.length) await git(repo, ['checkout', '-q', ...FROM_STDIN], pathspec(tracked));
  if (untracked && untracked.length) {
    const { shell } = require('electron');
    for (const rel of untracked) await shell.trashItem(safeJoin(repo, rel));
  }
}

async function discardAll(repo, includeUntracked) {
  if (await hasHead(repo)) await git(repo, ['reset', '--hard', '-q']);
  else await unstageAll(repo);
  if (includeUntracked) await git(repo, ['clean', '-fd', '-q']);
}

async function applyPatch(repo, patch, { cached = false, reverse = false } = {}) {
  const args = ['apply', '--recount', '--whitespace=nowarn'];
  if (cached) args.push('--cached');
  if (reverse) args.push('--reverse');
  args.push('-');
  await git(repo, args, { input: patch.endsWith('\n') ? patch : patch + '\n' });
}

async function commit(repo, message, { amend = false } = {}) {
  if (!String(message || '').trim()) throw new Error('El mensaje del commit no puede estar vacío.');
  const args = ['commit', '-F', '-'];
  if (amend) args.push('--amend');
  await git(repo, args, { input: message });
  return (await out(repo, ['rev-parse', 'HEAD'])).trim();
}

async function lastCommitMessage(repo) {
  if (!(await hasHead(repo))) return '';
  return (await out(repo, ['log', '-1', '--format=%B'])).replace(/\s+$/, '');
}

async function unmergedPaths(repo) {
  const r = await run(repo, ['diff', '--name-only', '-z', '--diff-filter=U']);
  return r.code === 0 ? r.stdout.toString('utf8').split('\0').filter(Boolean) : [];
}

// Commands that may stop with conflicts resolve with { conflicts: true } instead of failing.
async function conflictAware(repo, args, opts = {}) {
  const r = await run(repo, args, opts);
  if (r.cancelled) throw failure(r, args);
  const output = cleanOutput(r.stdout.toString('utf8') + '\n' + r.stderr);
  if (r.code === 0) return { conflicts: false, output };
  if ((await unmergedPaths(repo)).length) return { conflicts: true, output };
  throw failure(r, args);
}

async function checkRefName(repo, kind, name) {
  const full = kind === 'branch' ? name : `refs/tags/${name}`;
  const args = kind === 'branch' ? ['check-ref-format', '--branch', full] : ['check-ref-format', full];
  const r = await run(repo, args);
  if (r.code !== 0 || !name || name.startsWith('-')) throw new Error(`"${name}" no es un nombre válido.`);
}

async function checkout(repo, ref) {
  await git(repo, ['checkout', ref, '--']);
}

async function checkoutRemote(repo, remoteRef, localName) {
  await checkRefName(repo, 'branch', localName);
  await git(repo, ['checkout', '-b', localName, '--track', remoteRef, '--']);
}

async function checkoutDetached(repo, hash) {
  await git(repo, ['checkout', '--detach', hash, '--']);
}

async function createBranch(repo, name, startPoint, doCheckout) {
  await checkRefName(repo, 'branch', name);
  const args = doCheckout ? ['checkout', '-b', name] : ['branch', name];
  if (startPoint) args.push(startPoint);
  if (doCheckout) args.push('--');
  await git(repo, args);
}

async function renameBranch(repo, oldName, newName) {
  await checkRefName(repo, 'branch', newName);
  await git(repo, ['branch', '-m', oldName, newName]);
}

async function deleteBranch(repo, name, force) {
  await git(repo, ['branch', force ? '-D' : '-d', name]);
}

async function merge(repo, ref, { noFF = false } = {}) {
  const args = ['merge', '--no-edit'];
  if (noFF) args.push('--no-ff');
  args.push(ref);
  return conflictAware(repo, args);
}

async function rebase(repo, onto) {
  return conflictAware(repo, ['rebase', onto]);
}

async function cherryPick(repo, hash, isMerge) {
  return conflictAware(repo, isMerge ? ['cherry-pick', '-m', '1', hash] : ['cherry-pick', hash]);
}

async function revert(repo, hash, isMerge) {
  return conflictAware(repo, isMerge ? ['revert', '--no-edit', '-m', '1', hash] : ['revert', '--no-edit', hash]);
}

async function reset(repo, target, mode) {
  if (!['soft', 'mixed', 'hard'].includes(mode)) throw new Error('Modo de reset no válido.');
  await git(repo, ['reset', `--${mode}`, target, '--']);
}

const SEQ = { merge: 'merge', rebase: 'rebase', 'cherry-pick': 'cherry-pick', revert: 'revert' };

async function abortOperation(repo, kind) {
  if (!SEQ[kind]) throw new Error('Operación desconocida.');
  await git(repo, [SEQ[kind], '--abort']);
}

async function continueOperation(repo, kind) {
  if (kind === 'merge') return conflictAware(repo, ['commit', '--no-edit']);
  if (!SEQ[kind]) throw new Error('Operación desconocida.');
  return conflictAware(repo, [SEQ[kind], '--continue']);
}

async function skipRebase(repo) {
  return conflictAware(repo, ['rebase', '--skip']);
}

async function resolveConflict(repo, file, side) {
  if (side !== 'ours' && side !== 'theirs') throw new Error('Lado no válido.');
  await git(repo, ['checkout', `--${side}`, '--', file], { env: LITERAL });
  await stage(repo, [file]);
}

async function createTag(repo, name, target, message) {
  await checkRefName(repo, 'tag', name);
  const args = message ? ['tag', '-a', name, '-F', '-'] : ['tag', name];
  if (target) args.push(target);
  await git(repo, args, message ? { input: message } : {});
}

async function deleteTag(repo, name) {
  await git(repo, ['tag', '-d', name]);
}

async function fetch(repo, { remote = null, prune = true } = {}, opId = null) {
  const args = ['fetch', '--progress'];
  if (prune) args.push('--prune');
  args.push(remote || '--all');
  await git(repo, args, { opId });
}

async function pull(repo, { remote, branch, rebase: useRebase = false } = {}, opId = null) {
  const args = ['pull', '--progress', useRebase ? '--rebase' : '--no-rebase'];
  if (remote) {
    args.push(remote);
    if (branch) args.push(branch);
  }
  return conflictAware(repo, args, { opId });
}

async function push(repo, { remote, branch, remoteBranch, setUpstream = false, force = false, tags = false } = {}, opId = null) {
  if (!remote || !branch) throw new Error('Selecciona el remoto y la rama.');
  const args = ['push', '--progress'];
  if (setUpstream) args.push('--set-upstream');
  if (force) args.push('--force-with-lease');
  if (tags) args.push('--follow-tags');
  args.push(remote, `refs/heads/${branch}:refs/heads/${remoteBranch || branch}`);
  await git(repo, args, { opId });
}

async function deleteRemoteBranch(repo, remote, branch, opId = null) {
  await git(repo, ['push', '--progress', remote, '--delete', `refs/heads/${branch}`], { opId });
}

async function pushTag(repo, remote, name, opId = null) {
  await git(repo, ['push', '--progress', remote, `refs/tags/${name}`], { opId });
}

async function deleteRemoteTag(repo, remote, name, opId = null) {
  await git(repo, ['push', '--progress', remote, '--delete', `refs/tags/${name}`], { opId });
}

async function stashSave(repo, { message = '', untracked = false, keepIndex = false } = {}) {
  const args = ['stash', 'push'];
  if (untracked) args.push('--include-untracked');
  if (keepIndex) args.push('--keep-index');
  if (message) args.push('-m', message);
  await git(repo, args);
}

async function stashApply(repo, ref, pop) {
  return conflictAware(repo, ['stash', pop ? 'pop' : 'apply', ref]);
}

async function stashDrop(repo, ref) {
  await git(repo, ['stash', 'drop', ref]);
}

async function addRemote(repo, name, url) {
  if (!name || !url) throw new Error('Indica nombre y URL del remoto.');
  await git(repo, ['remote', 'add', '--', name, url]);
}

async function removeRemote(repo, name) {
  await git(repo, ['remote', 'remove', name]);
}

async function setIdentity(repo, { name, email, global = false }) {
  const scope = global ? '--global' : '--local';
  for (const [key, value] of [['user.name', name], ['user.email', email]]) {
    if (value && value.trim()) await git(repo, ['config', scope, key, value.trim()]);
    else await git(repo, ['config', scope, '--unset', key], { allowFail: true });
  }
}

function escapeIgnore(rel) {
  return rel.replace(/([*?[\]\\!#])/g, '\\$1').replace(/ $/, '\\ ');
}

function addToGitignore(repo, pattern) {
  const file = path.join(repo, '.gitignore');
  let cur = '';
  try {
    cur = fs.readFileSync(file, 'utf8');
  } catch { /* new file */ }
  if (cur.split(/\r?\n/).includes(pattern)) return;
  const nl = cur.includes('\r\n') ? '\r\n' : '\n';
  const prefix = cur && !cur.endsWith('\n') ? nl : '';
  fs.appendFileSync(file, prefix + pattern + nl);
}

async function ignorePaths(repo, paths, byExtension) {
  for (const rel of paths) {
    const ext = path.extname(rel);
    addToGitignore(repo, byExtension && ext ? `*${escapeIgnore(ext)}` : `/${escapeIgnore(rel.replace(/\\/g, '/'))}`);
  }
}

module.exports = {
  // plumbing
  setProgressSink,
  cancel,
  safeJoin,
  parseStatus,
  parseNameStatus,
  // api exposed to the renderer
  api: {
    version,
    resolveRepo,
    status,
    refs,
    log,
    commitDetails,
    commitFileDiff,
    workingDiff,
    readWorkingFile,
    identity,
    remotesInfo,
    lastCommitMessage,
    init,
    clone,
    stage,
    stageAll,
    unstage,
    unstageAll,
    discard,
    discardAll,
    applyPatch,
    commit,
    checkout,
    checkoutRemote,
    checkoutDetached,
    createBranch,
    renameBranch,
    deleteBranch,
    merge,
    rebase,
    cherryPick,
    revert,
    reset,
    abortOperation,
    continueOperation,
    skipRebase,
    resolveConflict,
    createTag,
    deleteTag,
    fetch,
    pull,
    push,
    deleteRemoteBranch,
    pushTag,
    deleteRemoteTag,
    stashSave,
    stashApply,
    stashDrop,
    addRemote,
    removeRemote,
    setIdentity,
    ignorePaths,
  },
};

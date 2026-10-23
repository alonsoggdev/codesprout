'use strict';
// Pure unified-diff helpers shared by the renderer and the Node test-suite.
(function (root) {
  const HUNK_RE = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/;

  function parseDiff(text) {
    const lines = String(text || '').split('\n');
    if (lines.length && lines[lines.length - 1] === '') lines.pop();
    const files = [];
    let file = null;
    let hunk = null;
    for (const line of lines) {
      if (line.startsWith('diff --git ') || line.startsWith('diff --cc ') || line.startsWith('diff --combined ')) {
        file = { header: [line], hunks: [], binary: false, combined: !line.startsWith('diff --git ') };
        files.push(file);
        hunk = null;
        continue;
      }
      if (!file) continue;
      if (line.startsWith('@@')) {
        const m = HUNK_RE.exec(line);
        if (m) {
          hunk = {
            header: line,
            oldStart: +m[1],
            oldCount: m[2] === undefined ? 1 : +m[2],
            newStart: +m[3],
            newCount: m[4] === undefined ? 1 : +m[4],
            context: m[5].trim(),
            lines: [],
          };
          file.hunks.push(hunk);
          continue;
        }
        if (!hunk) {
          file.header.push(line);
          continue;
        }
      }
      if (!hunk) {
        file.header.push(line);
        if (line.startsWith('Binary files ') || line === 'GIT binary patch') file.binary = true;
        continue;
      }
      hunk.lines.push(line);
    }
    for (const f of files) {
      f.isNew = f.header.some((l) => l.startsWith('new file mode'));
      f.isDeleted = f.header.some((l) => l.startsWith('deleted file mode'));
      const rename = f.header.find((l) => l.startsWith('rename from '));
      f.renamedFrom = rename ? rename.slice(12) : null;
      f.modeChange = f.header.some((l) => l.startsWith('old mode'));
    }
    return files;
  }

  // Line numbers for rendering.
  function numberHunk(hunk) {
    let o = hunk.oldStart;
    let n = hunk.newStart;
    return hunk.lines.map((l) => {
      const t = l[0];
      if (t === '+') return { type: 'add', old: null, new: n++ };
      if (t === '-') return { type: 'del', old: o++, new: null };
      if (t === '\\') return { type: 'meta', old: null, new: null };
      return { type: 'ctx', old: o++, new: n++ };
    });
  }

  // Builds a patch containing one hunk, optionally restricted to the selected line indexes.
  // reverse=false: patch will be applied forward (stage). Unselected '-' become context, unselected '+' are dropped.
  // reverse=true: patch will be applied with --reverse (unstage / discard). Unselected '+' become context,
  // unselected '-' are dropped. Returns null when nothing would change.
  function buildPatch(file, hunk, selected, reverse) {
    const body = [];
    let oldCount = 0;
    let newCount = 0;
    let changes = 0;
    let lastKept = false;
    hunk.lines.forEach((line, i) => {
      const t = line[0];
      const sel = !selected || selected.has(i);
      if (t === '-') {
        if (sel) {
          body.push(line);
          oldCount++;
          changes++;
          lastKept = true;
        } else if (!reverse) {
          body.push(' ' + line.slice(1));
          oldCount++;
          newCount++;
          lastKept = true;
        } else {
          lastKept = false;
        }
      } else if (t === '+') {
        if (sel) {
          body.push(line);
          newCount++;
          changes++;
          lastKept = true;
        } else if (reverse) {
          body.push(' ' + line.slice(1));
          oldCount++;
          newCount++;
          lastKept = true;
        } else {
          lastKept = false;
        }
      } else if (t === '\\') {
        if (lastKept) body.push(line);
      } else {
        body.push(line);
        oldCount++;
        newCount++;
        lastKept = true;
      }
    });
    if (!changes) return null;
    const header = `@@ -${hunk.oldStart},${oldCount} +${hunk.newStart},${newCount} @@${hunk.context ? ' ' + hunk.context : ''}`;
    return [...file.header, header, ...body].join('\n') + '\n';
  }

  // Whether partial (hunk / line) operations are safe for this file.
  function canPartial(file) {
    return !!file && !file.binary && !file.combined && !file.isNew && !file.isDeleted && file.hunks.length > 0;
  }

  const api = { parseDiff, numberHunk, buildPatch, canPartial };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.DiffCore = api;
})(typeof window !== 'undefined' ? window : globalThis);

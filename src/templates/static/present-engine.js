/*
 * Interactive presentation engine.
 *
 * One document model, two authoring views: the visual builder edits the JSON
 * document directly, and the plain-text script is parsed into / generated from
 * that same document (PresentEngine.parseScript / PresentEngine.toScript).
 * The player drives a document with PresentEngine.Runtime.
 *
 * Document shape (format 1):
 *   { format, start, vars: {name: value}, pages: [page] }
 *   page = { id, text, media, media_once, audio, audio_loop,
 *            set: [assign], branches: [{if, goto}], buttons: [button],
 *            timer: null | {seconds, goto, hidden, set}, end }
 *   button = { label, goto: [target], if, set: [assign] }
 *   assign = { var, op, expr }            op: = += -= *= /=
 *   target = page id | glob (part*) | "next" | "end"
 */
(function (global) {
  'use strict';

  const FORMAT = 1;
  const ID_RE = /^[A-Za-z0-9_-]{1,64}$/;
  const TARGET_RE = /^[A-Za-z0-9_*-]{1,64}$/;
  const VAR_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
  const RESERVED_TARGETS = ['next', 'end'];
  const MAX_REDIRECTS = 100;

  const EXTENSIONS = {
    image: ['webp', 'png', 'jpg', 'jpeg', 'gif', 'avif', 'bmp'],
    video: ['mp4', 'webm', 'm4v', 'ogv', 'mov'],
    audio: ['mp3', 'ogg', 'oga', 'wav', 'm4a', 'aac', 'flac', 'opus', 'weba'],
  };

  function mediaKind(filename) {
    const ext = String(filename || '').split('.').pop().toLowerCase();
    for (const kind of Object.keys(EXTENSIONS)) {
      if (EXTENSIONS[kind].includes(ext)) return kind;
    }
    return null;
  }

  // ─── Expressions ──────────────────────────────────────────────────────────
  // A tiny, sandboxed expression language (never uses eval):
  //   numbers, "strings", true/false, variables, + - * / %, comparisons
  //   (== != < <= > >=, a lone = also means ==), and/or/not (&& || !),
  //   parentheses and a few functions (see FUNCTIONS).

  class ExprError extends Error {}

  const TOKEN_RE = /\s*(?:(\d+(?:\.\d+)?|\.\d+)|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')|([A-Za-z_][A-Za-z0-9_]*)|(==|!=|<=|>=|&&|\|\||[-+*/%<>!(),=]))/y;

  function tokenize(src) {
    const tokens = [];
    TOKEN_RE.lastIndex = 0;
    let pos = 0;
    while (pos < src.length) {
      if (/^\s*$/.test(src.slice(pos))) break;
      TOKEN_RE.lastIndex = pos;
      const m = TOKEN_RE.exec(src);
      if (!m) throw new ExprError('Unexpected character "' + src.slice(pos).trim()[0] + '"');
      pos = TOKEN_RE.lastIndex;
      if (m[1] !== undefined) tokens.push({ t: 'num', v: parseFloat(m[1]) });
      else if (m[2] !== undefined) tokens.push({ t: 'str', v: m[2].slice(1, -1).replace(/\\(.)/g, '$1') });
      else if (m[3] !== undefined) {
        const w = m[3];
        const lw = w.toLowerCase();
        if (lw === 'and') tokens.push({ t: 'op', v: '&&' });
        else if (lw === 'or') tokens.push({ t: 'op', v: '||' });
        else if (lw === 'not') tokens.push({ t: 'op', v: '!' });
        else if (lw === 'true') tokens.push({ t: 'lit', v: true });
        else if (lw === 'false') tokens.push({ t: 'lit', v: false });
        else tokens.push({ t: 'id', v: w });
      } else tokens.push({ t: 'op', v: m[4] === '=' ? '==' : m[4] });
    }
    return tokens;
  }

  function parseExpr(src) {
    const tokens = tokenize(String(src));
    let i = 0;
    const peek = () => tokens[i];
    const isOp = (v) => tokens[i] && tokens[i].t === 'op' && tokens[i].v === v;
    const expect = (v) => {
      if (!isOp(v)) throw new ExprError('Expected "' + v + '"');
      i++;
    };

    function or() {
      let a = and();
      while (isOp('||')) { i++; a = ['or', a, and()]; }
      return a;
    }
    function and() {
      let a = not();
      while (isOp('&&')) { i++; a = ['and', a, not()]; }
      return a;
    }
    function not() {
      if (isOp('!')) { i++; return ['not', not()]; }
      return cmp();
    }
    function cmp() {
      const a = add();
      const tok = peek();
      if (tok && tok.t === 'op' && ['==', '!=', '<', '<=', '>', '>='].includes(tok.v)) {
        i++;
        return ['bin', tok.v, a, add()];
      }
      return a;
    }
    function add() {
      let a = mul();
      while (isOp('+') || isOp('-')) { const op = tokens[i++].v; a = ['bin', op, a, mul()]; }
      return a;
    }
    function mul() {
      let a = unary();
      while (isOp('*') || isOp('/') || isOp('%')) { const op = tokens[i++].v; a = ['bin', op, a, unary()]; }
      return a;
    }
    function unary() {
      if (isOp('-')) { i++; return ['neg', unary()]; }
      if (isOp('+')) { i++; return unary(); }
      return primary();
    }
    function primary() {
      const tok = tokens[i++];
      if (!tok) throw new ExprError('Expression ends unexpectedly');
      if (tok.t === 'num' || tok.t === 'str' || tok.t === 'lit') return ['lit', tok.v];
      if (tok.t === 'id') {
        if (isOp('(')) {
          i++;
          const name = tok.v.toLowerCase();
          if (!FUNCTIONS[name]) throw new ExprError('Unknown function "' + tok.v + '"');
          const args = [];
          if (!isOp(')')) {
            do {
              // visited(intro) — bare page names are allowed as page arguments
              const t = peek();
              const next = tokens[i + 1];
              if (PAGE_FUNCTIONS.includes(name) && t && t.t === 'id' && next && next.t === 'op' && (next.v === ')' || next.v === ',')) {
                i++;
                args.push(['lit', t.v]);
              } else {
                args.push(or());
              }
            } while (isOp(',') && ++i);
          }
          expect(')');
          return ['call', name, args];
        }
        return ['var', tok.v];
      }
      if (tok.t === 'op' && tok.v === '(') {
        const e = or();
        expect(')');
        return e;
      }
      throw new ExprError('Unexpected "' + tok.v + '"');
    }

    if (!tokens.length) throw new ExprError('Empty expression');
    const ast = or();
    if (i < tokens.length) throw new ExprError('Unexpected "' + tokens[i].v + '"');
    return ast;
  }

  const num = (v) => {
    if (typeof v === 'number') return v;
    if (typeof v === 'boolean') return v ? 1 : 0;
    const n = parseFloat(v);
    return Number.isFinite(n) ? n : 0;
  };
  const randInt = (a, b) => {
    a = Math.ceil(num(a)); b = Math.floor(num(b));
    if (b < a) [a, b] = [b, a];
    return a + Math.floor(Math.random() * (b - a + 1));
  };

  const PAGE_FUNCTIONS = ['visited', 'visits'];
  const FUNCTIONS = {
    // random() → 0..1, random(n) → 1..n, random(a, b) → a..b (whole numbers)
    random: (ctx, a, b) => a === undefined ? Math.random() : b === undefined ? randInt(1, a) : randInt(a, b),
    chance: (ctx, pct) => Math.random() * 100 < num(pct),
    pick: (ctx, ...items) => items.length ? items[Math.floor(Math.random() * items.length)] : 0,
    min: (ctx, ...xs) => Math.min(...xs.map(num)),
    max: (ctx, ...xs) => Math.max(...xs.map(num)),
    abs: (ctx, x) => Math.abs(num(x)),
    round: (ctx, x) => Math.round(num(x)),
    floor: (ctx, x) => Math.floor(num(x)),
    ceil: (ctx, x) => Math.ceil(num(x)),
    visited: (ctx, page) => (ctx.visits[page] || 0) > 0,
    visits: (ctx, page) => ctx.visits[page] || 0,
  };

  function evalAst(ast, ctx) {
    switch (ast[0]) {
      case 'lit': return ast[1];
      case 'var': return Object.prototype.hasOwnProperty.call(ctx.vars, ast[1]) ? ctx.vars[ast[1]] : 0;
      case 'not': return !evalAst(ast[1], ctx);
      case 'neg': return -num(evalAst(ast[1], ctx));
      case 'and': return evalAst(ast[1], ctx) && evalAst(ast[2], ctx);
      case 'or': return evalAst(ast[1], ctx) || evalAst(ast[2], ctx);
      case 'call': return FUNCTIONS[ast[1]](ctx, ...ast[2].map((a) => evalAst(a, ctx)));
      case 'bin': {
        const a = evalAst(ast[2], ctx);
        const b = evalAst(ast[3], ctx);
        switch (ast[1]) {
          case '+': return (typeof a === 'string' || typeof b === 'string') ? String(a) + String(b) : num(a) + num(b);
          case '-': return num(a) - num(b);
          case '*': return num(a) * num(b);
          case '/': return num(b) === 0 ? 0 : num(a) / num(b);
          case '%': return num(b) === 0 ? 0 : num(a) % num(b);
          // eslint-disable-next-line eqeqeq
          case '==': return typeof a === typeof b ? a === b : a == b;
          // eslint-disable-next-line eqeqeq
          case '!=': return typeof a === typeof b ? a !== b : a != b;
          case '<': return a < b;
          case '<=': return a <= b;
          case '>': return a > b;
          case '>=': return a >= b;
        }
      }
    }
    throw new ExprError('Bad expression');
  }

  const compiled = new Map();
  function compile(src) {
    const key = String(src);
    if (!compiled.has(key)) {
      try {
        compiled.set(key, { ast: parseExpr(key) });
      } catch (e) {
        compiled.set(key, { error: e.message });
      }
    }
    const c = compiled.get(key);
    if (c.error) throw new ExprError(c.error);
    return c.ast;
  }

  function evaluate(src, ctx) {
    return evalAst(compile(src), ctx || { vars: {}, visits: {} });
  }

  function exprError(src) {
    try { compile(src); return null; } catch (e) { return e.message; }
  }

  // ─── Assignments ("score += 1; name = "x"") ──────────────────────────────

  function splitOutsideQuotes(src, sep) {
    const parts = [];
    let cur = '';
    let quote = null;
    for (let i = 0; i < src.length; i++) {
      const c = src[i];
      if (quote) {
        cur += c;
        if (c === '\\' && i + 1 < src.length) cur += src[++i];
        else if (c === quote) quote = null;
      } else if (c === '"' || c === "'") {
        quote = c;
        cur += c;
      } else if (c === sep) {
        parts.push(cur);
        cur = '';
      } else cur += c;
    }
    parts.push(cur);
    return parts.map((p) => p.trim()).filter(Boolean);
  }

  const ASSIGN_RE = /^([A-Za-z_][A-Za-z0-9_]*)\s*(\+=|-=|\*=|\/=|=)(?!=)\s*(.+)$/;

  /** "a = 1; b += 2" → {list, errors} */
  function parseAssignments(src) {
    const list = [];
    const errors = [];
    for (const part of splitOutsideQuotes(String(src || ''), ';')) {
      const m = ASSIGN_RE.exec(part);
      if (!m) { errors.push('"' + part + '" should look like name = value'); continue; }
      const err = exprError(m[3]);
      if (err) errors.push('In "' + part + '": ' + err);
      list.push({ var: m[1], op: m[2], expr: m[3].trim() });
    }
    return { list, errors };
  }

  function assignmentsToString(list) {
    return (list || []).map((a) => a.var + ' ' + (a.op || '=') + ' ' + a.expr).join('; ');
  }

  function applyAssignments(list, ctx) {
    for (const a of list || []) {
      let v = evaluate(a.expr, ctx);
      const cur = Object.prototype.hasOwnProperty.call(ctx.vars, a.var) ? ctx.vars[a.var] : 0;
      switch (a.op) {
        case '+=': v = (typeof cur === 'string' || typeof v === 'string') ? String(cur) + String(v) : num(cur) + num(v); break;
        case '-=': v = num(cur) - num(v); break;
        case '*=': v = num(cur) * num(v); break;
        case '/=': v = num(v) === 0 ? 0 : num(cur) / num(v); break;
      }
      ctx.vars[a.var] = v;
    }
  }

  // ─── Targets ("part2, part3" / "ending*") ────────────────────────────────

  function parseTargets(src) {
    return String(src || '').split(/[,|]/).map((s) => s.trim()).filter(Boolean);
  }

  function globToRegex(glob) {
    return new RegExp('^' + glob.split('*').map((s) => s.replace(/[.+?^${}()|[\]\\-]/g, '\\$&')).join('.*') + '$');
  }

  // ─── Timer specs ("30", "10-20s", "2m", "media") ─────────────────────────

  function parseTimerSpec(src) {
    const s = String(src || '').trim().toLowerCase();
    if (s === 'media') return { spec: 'media' };
    const m = /^(\d+(?:\.\d+)?)(?:\s*-\s*(\d+(?:\.\d+)?))?\s*(s|sec|secs|seconds?|m|min|mins|minutes?)?$/.exec(s);
    if (!m) return { error: 'Timer should be seconds like 30, a range like 10-20, or "media"' };
    const mult = m[3] && m[3][0] === 'm' ? 60 : 1;
    const a = +(parseFloat(m[1]) * mult).toFixed(2);
    const b = m[2] !== undefined ? +(parseFloat(m[2]) * mult).toFixed(2) : null;
    if (b !== null && b < a) return { error: 'Timer range should go from low to high' };
    return { spec: b === null ? String(a) : a + '-' + b };
  }

  // ─── Document helpers ────────────────────────────────────────────────────

  function newPage(id) {
    return {
      id, text: '', media: '', media_once: false, audio: '', audio_loop: false,
      set: [], branches: [], buttons: [], timer: null, end: false,
    };
  }

  function newButton() {
    return { label: 'Continue', goto: [], if: '', set: [] };
  }

  function newTimer() {
    return { seconds: '10', goto: [], hidden: false, set: [] };
  }

  const arr = (v) => Array.isArray(v) ? v : [];
  const str = (v) => (v === undefined || v === null) ? '' : String(v);
  const targets = (v) => Array.isArray(v) ? v.map(str).filter(Boolean) : parseTargets(v);
  const assigns = (v) => arr(v).filter((a) => a && a.var).map((a) => ({ var: str(a.var), op: a.op || '=', expr: str(a.expr) }));

  /** Fill in defaults so the rest of the code can trust the shape. */
  function normalize(raw) {
    const d = raw && typeof raw === 'object' ? raw : {};
    const doc = {
      format: FORMAT,
      start: str(d.start),
      vars: {},
      pages: arr(d.pages).map((p) => ({
        id: str(p.id),
        text: str(p.text),
        media: str(p.media),
        media_once: !!p.media_once,
        audio: str(p.audio),
        audio_loop: !!p.audio_loop,
        set: assigns(p.set),
        branches: arr(p.branches).map((b) => ({ if: str(b.if), goto: targets(b.goto) })),
        buttons: arr(p.buttons).map((b) => ({ label: str(b.label), goto: targets(b.goto), if: str(b.if), set: assigns(b.set) })),
        timer: p.timer ? { seconds: str(p.timer.seconds || '10'), goto: targets(p.timer.goto), hidden: !!p.timer.hidden, set: assigns(p.timer.set) } : null,
        end: !!p.end,
      })),
    };
    for (const [k, v] of Object.entries(d.vars || {})) {
      doc.vars[k] = (typeof v === 'number' || typeof v === 'boolean') ? v : str(v);
    }
    if (!doc.pages.length) doc.pages.push(newPage('start'));
    if (!doc.start) doc.start = doc.pages[0].id;
    return doc;
  }

  /**
   * Structural checks shared by the builder and the player.
   * mediaNames (optional) is the list of uploaded files, used to warn about
   * references to files that don't exist.
   */
  function validate(doc, mediaNames) {
    const errors = [];
    const warnings = [];
    const ids = new Set();
    const where = (p) => 'Page "' + p.id + '": ';

    for (const k of Object.keys(doc.vars)) {
      if (!VAR_RE.test(k)) errors.push('Variable name "' + k + '" should use letters, numbers and _');
    }
    doc.pages.forEach((p, i) => {
      if (!ID_RE.test(p.id)) errors.push('Page ' + (i + 1) + ' has an invalid id "' + p.id + '" (letters, numbers, - and _ only)');
      else if (RESERVED_TARGETS.includes(p.id)) errors.push('"' + p.id + '" is reserved and can\'t be a page id');
      else if (ids.has(p.id)) errors.push('Two pages are named "' + p.id + '"');
      ids.add(p.id);
    });
    if (!ids.has(doc.start)) errors.push('Start page "' + doc.start + '" does not exist');

    const idList = [...ids];
    const checkTargets = (p, list, what) => {
      for (const t of list) {
        if (RESERVED_TARGETS.includes(t)) continue;
        if (!TARGET_RE.test(t)) { errors.push(where(p) + what + ' goes to "' + t + '", which is not a valid page name'); continue; }
        if (t.includes('*')) {
          const re = globToRegex(t);
          if (!idList.some((id) => re.test(id))) errors.push(where(p) + what + ' pattern "' + t + '" matches no pages');
        } else if (!ids.has(t)) errors.push(where(p) + what + ' goes to missing page "' + t + '"');
      }
    };
    const checkExpr = (p, src, what) => {
      if (!src) return;
      const e = exprError(src);
      if (e) errors.push(where(p) + what + ' condition "' + src + '": ' + e);
    };
    const checkAssigns = (p, list, what) => {
      for (const a of list) {
        if (!VAR_RE.test(a.var)) errors.push(where(p) + what + ' sets invalid variable "' + a.var + '"');
        const e = exprError(a.expr);
        if (e) errors.push(where(p) + what + ' "' + a.var + ' ' + a.op + ' ' + a.expr + '": ' + e);
      }
    };
    const media = mediaNames ? new Set(mediaNames) : null;
    const checkFile = (p, file, kinds, what) => {
      if (!file || file === 'stop') return;
      const k = mediaKind(file);
      if (!k || !kinds.includes(k)) errors.push(where(p) + what + ' "' + file + '" is not a supported ' + kinds.join('/') + ' file');
      else if (media && !media.has(file)) warnings.push(where(p) + what + ' "' + file + '" has not been uploaded');
    };

    for (const p of doc.pages) {
      checkFile(p, p.media, ['image', 'video'], 'media');
      checkFile(p, p.audio, ['audio'], 'audio');
      checkAssigns(p, p.set, 'On enter');
      p.branches.forEach((b, i) => {
        checkExpr(p, b.if, 'Jump ' + (i + 1));
        if (!b.goto.length) errors.push(where(p) + 'jump ' + (i + 1) + ' has no destination');
        checkTargets(p, b.goto, 'Jump ' + (i + 1));
      });
      p.buttons.forEach((b, i) => {
        const what = 'Button "' + (b.label || i + 1) + '"';
        if (!b.label.trim()) warnings.push(where(p) + 'button ' + (i + 1) + ' has no label');
        checkExpr(p, b.if, what);
        checkAssigns(p, b.set, what);
        checkTargets(p, b.goto, what);
      });
      if (p.timer) {
        const t = parseTimerSpec(p.timer.seconds);
        if (t.error) errors.push(where(p) + t.error);
        else if (t.spec === 'media' && !p.media && !p.audio) warnings.push(where(p) + 'timer waits for media, but the page has none');
        else if (t.spec === 'media' && mediaKind(p.media) === 'image' && !p.audio) warnings.push(where(p) + 'timer waits for media, but images never finish');
        checkAssigns(p, p.timer.set, 'Timer');
        checkTargets(p, p.timer.goto, 'Timer');
      }
      if (p.end && (p.buttons.length || p.timer)) warnings.push(where(p) + 'is an end page, so its buttons and timer are ignored');
    }
    return { errors, warnings };
  }

  // ─── Script ⇄ document ───────────────────────────────────────────────────

  const KEYWORD_RE = /^(media|image|video|audio|set|goto|button|timer|start)\s*:|^(if\s.+->|end$|var\s|==|#|\\)/i;

  function parseClauses(rest) {
    // Pull [if ...], [set ...] and [hidden] clauses off the end of a line.
    const out = { if: '', set: [], hidden: false, errors: [] };
    const re = /\[\s*(if|set|hidden)\b([^\]]*)\]/gi;
    const main = rest.replace(re, (_, kind, body) => {
      kind = kind.toLowerCase();
      if (kind === 'if') out.if = body.trim();
      else if (kind === 'set') {
        const r = parseAssignments(body);
        out.set = out.set.concat(r.list);
        out.errors.push(...r.errors);
      } else out.hidden = true;
      return '';
    }).trim();
    return { main, ...out };
  }

  function parseLiteral(src) {
    const v = evaluate(src, { vars: {}, visits: {} });
    return v;
  }

  /** Script text → {doc, errors: [{line, message}]} */
  function parseScript(text) {
    const doc = { format: FORMAT, start: '', vars: {}, pages: [] };
    const errors = [];
    let page = null;
    let textLines = [];

    const flush = () => {
      if (page) {
        while (textLines.length && !textLines[0].trim()) textLines.shift();
        while (textLines.length && !textLines[textLines.length - 1].trim()) textLines.pop();
        page.text = textLines.join('\n');
      }
      textLines = [];
    };

    String(text).split(/\r?\n/).forEach((raw, idx) => {
      const n = idx + 1;
      const t = raw.trim();
      const err = (message) => errors.push({ line: n, message });
      let m;

      if (t.startsWith('==')) {
        flush();
        const id = t.replace(/^=+/, '').replace(/=+$/, '').trim();
        if (!ID_RE.test(id)) err('Page name "' + id + '" should use letters, numbers, - and _ only');
        page = newPage(id);
        doc.pages.push(page);
        return;
      }
      if (t.startsWith('#')) return;

      if (!page) {
        if (!t) return;
        if ((m = /^start\s*:\s*(.+)$/i.exec(t))) { doc.start = m[1].trim(); return; }
        if ((m = /^var\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/i.exec(t))) {
          try { doc.vars[m[1]] = parseLiteral(m[2]); } catch (e) { err('Variable "' + m[1] + '": ' + e.message); }
          return;
        }
        err('Before the first page, only "var name = value" and "start: page" are allowed. Start a page with "== name".');
        return;
      }

      if (t.startsWith('\\')) { textLines.push(t.slice(1)); return; }

      if ((m = /^(media|image|video)\s*:\s*(.*)$/i.exec(t))) {
        const [file, ...flags] = m[2].trim().split(/\s+/);
        page.media = file === 'none' ? '' : (file || '');
        page.media_once = flags.some((f) => /^\[?once\]?$/i.test(f));
        return;
      }
      if ((m = /^audio\s*:\s*(.*)$/i.exec(t))) {
        const [file, ...flags] = m[1].trim().split(/\s+/);
        page.audio = file === 'none' ? '' : (file || '');
        page.audio_loop = flags.some((f) => /^\[?loop\]?$/i.test(f));
        return;
      }
      if ((m = /^set\s*:\s*(.*)$/i.exec(t))) {
        const r = parseAssignments(m[1]);
        page.set.push(...r.list);
        r.errors.forEach(err);
        return;
      }
      if ((m = /^if\s+(.+?)\s*->\s*(.+)$/i.exec(t))) {
        const e = exprError(m[1]);
        if (e) err('Condition "' + m[1] + '": ' + e);
        page.branches.push({ if: m[1].trim(), goto: parseTargets(m[2]) });
        return;
      }
      if ((m = /^goto\s*:\s*(.+)$/i.exec(t))) {
        page.branches.push({ if: '', goto: parseTargets(m[1]) });
        return;
      }
      if ((m = /^button\s*:\s*(.*)$/i.exec(t))) {
        const c = parseClauses(m[1]);
        c.errors.forEach(err);
        const arrow = c.main.lastIndexOf('->');
        const label = (arrow >= 0 ? c.main.slice(0, arrow) : c.main).trim();
        const gotoList = arrow >= 0 ? parseTargets(c.main.slice(arrow + 2)) : [];
        if (c.if) { const e = exprError(c.if); if (e) err('Condition "' + c.if + '": ' + e); }
        page.buttons.push({ label, goto: gotoList, if: c.if, set: c.set });
        return;
      }
      if ((m = /^timer\s*:\s*(.*)$/i.exec(t))) {
        const c = parseClauses(m[1]);
        c.errors.forEach(err);
        const arrow = c.main.indexOf('->');
        let spec = (arrow >= 0 ? c.main.slice(0, arrow) : c.main).trim();
        let hidden = c.hidden;
        if (/\bhidden$/i.test(spec)) { hidden = true; spec = spec.replace(/\s*hidden$/i, ''); }
        let rest = arrow >= 0 ? c.main.slice(arrow + 2).trim() : '';
        if (/\bhidden$/i.test(rest)) { hidden = true; rest = rest.replace(/\s*hidden$/i, ''); }
        const ts = parseTimerSpec(spec);
        if (ts.error) err(ts.error);
        page.timer = { seconds: ts.spec || spec, goto: parseTargets(rest), hidden, set: c.set };
        return;
      }
      if (/^end$/i.test(t)) { page.end = true; return; }

      textLines.push(t);
    });
    flush();

    if (!doc.pages.length) errors.push({ line: 1, message: 'Add at least one page, e.g. "== intro"' });
    if (!doc.start && doc.pages.length) doc.start = doc.pages[0].id;
    return { doc, errors };
  }

  function literal(v) {
    if (typeof v === 'number' || typeof v === 'boolean') return String(v);
    return '"' + String(v).replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
  }

  function clauseText(item) {
    let s = '';
    if (item.if) s += ' [if ' + item.if + ']';
    if (item.set && item.set.length) s += ' [set ' + assignmentsToString(item.set) + ']';
    return s;
  }

  /** Document → script text. parseScript(toScript(doc)) round-trips. */
  function toScript(doc) {
    const out = [];
    if (doc.pages.length && doc.start && doc.start !== doc.pages[0].id) out.push('start: ' + doc.start);
    for (const [k, v] of Object.entries(doc.vars)) out.push('var ' + k + ' = ' + literal(v));
    if (out.length) out.push('');

    for (const p of doc.pages) {
      out.push('== ' + p.id);
      if (p.media) out.push('media: ' + p.media + (p.media_once ? ' once' : ''));
      if (p.audio) out.push('audio: ' + p.audio + (p.audio_loop ? ' loop' : ''));
      for (const a of p.set) out.push('set: ' + assignmentsToString([a]));
      for (const b of p.branches) {
        out.push(b.if ? 'if ' + b.if + ' -> ' + b.goto.join(', ') : 'goto: ' + b.goto.join(', '));
      }
      if (p.text) {
        for (const line of p.text.split('\n')) {
          out.push(KEYWORD_RE.test(line.trim()) ? '\\' + line : line);
        }
      }
      for (const b of p.buttons) {
        out.push('button: ' + b.label + (b.goto.length ? ' -> ' + b.goto.join(', ') : '') + clauseText(b));
      }
      if (p.timer) {
        const t = p.timer;
        out.push('timer: ' + t.seconds + (t.goto.length ? ' -> ' + t.goto.join(', ') : '') +
          (t.hidden ? ' [hidden]' : '') + clauseText({ set: t.set }));
      }
      if (p.end) out.push('end');
      out.push('');
    }
    return out.join('\n');
  }

  // ─── Text rendering ──────────────────────────────────────────────────────

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }

  function formatValue(v) {
    if (typeof v === 'number' && !Number.isInteger(v)) return String(+v.toFixed(2));
    return String(v);
  }

  /** Replace {expression} with its value; unknown/invalid ones are left as typed. */
  function interpolate(text, ctx) {
    return String(text).replace(/\{([^{}\n]+)\}/g, (whole, expr) => {
      try { return formatValue(evaluate(expr, ctx)); } catch (e) { return whole; }
    });
  }

  /** Page text → safe HTML: {expr} values, **bold**, *italic*, paragraphs. */
  function renderText(text, ctx) {
    const html = escapeHtml(interpolate(text, ctx))
      .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
      .replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<em>$2</em>');
    return html.split(/\n\s*\n/).map((p) => '<p>' + p.replace(/\n/g, '<br>') + '</p>').join('');
  }

  // ─── Runtime ─────────────────────────────────────────────────────────────

  class Runtime {
    constructor(doc) {
      this.doc = normalize(doc);
      this.byId = new Map(this.doc.pages.map((p) => [p.id, p]));
      this.reset();
    }

    reset() {
      this.vars = JSON.parse(JSON.stringify(this.doc.vars));
      this.visits = {};
      this.current = null;
    }

    get ctx() { return { vars: this.vars, visits: this.visits }; }

    test(cond) {
      if (!cond) return true;
      try { return !!evaluate(cond, this.ctx); } catch (e) { return false; }
    }

    apply(list) {
      try { applyAssignments(list, this.ctx); } catch (e) { /* bad assignment: skip */ }
    }

    nextId(fromId) {
      const i = this.doc.pages.findIndex((p) => p.id === fromId);
      return i >= 0 && i + 1 < this.doc.pages.length ? this.doc.pages[i + 1].id : 'end';
    }

    /** Pick a destination: empty → next page; several → random; globs expand. */
    resolve(list, fromId) {
      if (!list || !list.length) return this.nextId(fromId);
      const options = [];
      for (const t of list) {
        if (t === 'next') options.push(this.nextId(fromId));
        else if (t === 'end') options.push('end');
        else if (t.includes('*')) {
          const re = globToRegex(t);
          const matches = this.doc.pages.map((p) => p.id).filter((id) => re.test(id));
          const others = matches.filter((id) => id !== fromId);
          options.push(...(others.length ? others : matches));
        } else options.push(t);
      }
      return options.length ? options[Math.floor(Math.random() * options.length)] : 'end';
    }

    /**
     * Enter a page, following any automatic jumps. Returns the page to show,
     * or null when the presentation has finished.
     */
    enter(id) {
      for (let hops = 0; hops < MAX_REDIRECTS; hops++) {
        if (id === 'end') { this.current = null; return null; }
        const page = this.byId.get(id);
        if (!page) throw new Error('Page "' + id + '" does not exist');
        this.visits[id] = (this.visits[id] || 0) + 1;
        this.apply(page.set);
        const jump = page.branches.find((b) => this.test(b.if));
        if (jump) { id = this.resolve(jump.goto, page.id); continue; }
        this.current = page;
        return page;
      }
      throw new Error('Too many automatic jumps in a row — check for a loop');
    }

    start(pageId) {
      this.reset();
      return this.enter(pageId && this.byId.has(pageId) ? pageId : this.doc.start);
    }

    buttons() {
      return this.current ? this.current.buttons.filter((b) => this.test(b.if)) : [];
    }

    press(button) {
      const from = this.current.id;
      this.apply(button.set);
      return this.enter(this.resolve(button.goto, from));
    }

    timerSeconds() {
      const t = this.current && this.current.timer;
      if (!t) return null;
      const spec = parseTimerSpec(t.seconds).spec;
      if (!spec) return null;
      if (spec === 'media') return 'media';
      const [a, b] = spec.split('-').map(Number);
      return b === undefined ? a : a + Math.random() * (b - a);
    }

    timerFire() {
      const from = this.current.id;
      this.apply(this.current.timer.set);
      return this.enter(this.resolve(this.current.timer.goto, from));
    }

    text() {
      return this.current ? renderText(this.current.text, this.ctx) : '';
    }
  }

  global.PresentEngine = {
    FORMAT, EXTENSIONS, ID_RE, RESERVED_TARGETS,
    mediaKind, evaluate, exprError, parseAssignments, assignmentsToString,
    parseTargets, parseTimerSpec, newPage, newButton, newTimer,
    normalize, validate, parseScript, toScript, renderText, escapeHtml, Runtime,
  };
})(typeof window !== 'undefined' ? window : globalThis);

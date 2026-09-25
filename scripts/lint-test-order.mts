// Lint rule: unit tests must be sorted alphabetically.
//
// Neither oxlint (jest/vitest plugins) nor Clippy provides a test-ordering rule,
// so this script is the rule for both languages. It runs as part of `bun run lint`
// and the commit hook (see `scripts/hook-checks.sh`). Dependency-free on purpose:
// TypeScript 7 no longer exposes the classic compiler API, so the TS side uses a
// small purpose-built tokenizer plus paren matching instead of a full parser.
//
// Scope:
// - TypeScript: `tests/*.test.ts`. Within each scope (file top level and every
//   `describe` body), `describe`/`test`/`it` blocks (including `.each`/`.skip`
//   variants) must be ordered by title in Unicode code-point order.
//   `test.each` data rows are not titles and are ignored. Calls with dynamic
//   (non-literal) titles are skipped.
// - Rust: `src-tauri/src/tests/**/*.rs`. `#[test]`/`#[tokio::test]` functions in
//   each file must be ordered by function name in byte order. Non-test helpers
//   are ignored and stay fixed; tests are permuted among test slots.
//
// Usage:
// - `bun scripts/lint-test-order.mts --check` (default): exit 1 on violation.
// - `bun scripts/lint-test-order.mts --fix`: reorder in place, then re-check.
import { readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root: string = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tsTestDir: string = join(root, "tests");
const rustTestDir: string = join(root, "src-tauri", "src", "tests");

type Violation = {
  file: string;
  line: number;
  message: string;
};

type Mode = "check" | "fix";

function parseMode(args: ReadonlyArray<string>): Mode {
  const wantsFix: boolean = args.includes("--fix");
  const wantsCheck: boolean = args.includes("--check");
  if (wantsFix && wantsCheck) {
    throw new Error("Pass only one of --check or --fix");
  }
  for (const arg of args) {
    if (arg !== "--fix" && arg !== "--check") {
      throw new Error(`Unknown argument: ${arg}. Usage: lint-test-order.mts [--check|--fix]`);
    }
  }
  return wantsFix ? "fix" : "check";
}

function lineOf(text: string, offset: number): number {
  let line: number = 1;
  for (let i: number = 0; i < offset; i++) {
    if (text[i] === "\n") {
      line++;
    }
  }
  return line;
}

function isSortedPair(a: string, b: string): boolean {
  return a <= b;
}

// ---------------------------------------------------------------------------
// TypeScript
// ---------------------------------------------------------------------------

const TEST_ROOTS: ReadonlySet<string> = new Set(["describe", "test", "it"]);

type TsToken =
  | { kind: "ident"; value: string; start: number; end: number }
  | { kind: "string"; value: string; start: number; end: number }
  | { kind: "template"; start: number; end: number }
  | { kind: "regex"; start: number; end: number }
  | { kind: "number"; start: number; end: number }
  | { kind: "punct"; value: string; start: number; end: number };

function isIdentStart(c: string): boolean {
  return /[A-Za-z_$]/.test(c);
}

function isIdentPart(c: string): boolean {
  return /[\w$]/.test(c);
}

function isDigit(c: string): boolean {
  return /[0-9]/.test(c);
}

// Tokens that end an operand, so a following `/` starts division, not regex.
function endsOperand(token: TsToken | undefined): boolean {
  if (token === undefined) {
    return false;
  }
  if (token.kind === "ident" || token.kind === "string" || token.kind === "number") {
    return true;
  }
  if (token.kind === "regex" || token.kind === "template") {
    return true;
  }
  return (
    token.kind === "punct" &&
    (token.value === ")" ||
      token.value === "]" ||
      token.value === "}" ||
      token.value === "++" ||
      token.value === "--")
  );
}

// Scans a template literal starting at the opening backtick. Returns the end
// offset (exclusive). Emits `hasSubstitution = true` when `${...}` occurs.
function scanTemplate(text: string, start: number): { end: number; hasSubstitution: boolean } {
  let i: number = start + 1;
  let hasSubstitution: boolean = false;
  while (i < text.length) {
    const c: string = text[i] as string;
    if (c === "\\") {
      i += 2;
      continue;
    }
    if (c === "`") {
      return { end: i + 1, hasSubstitution };
    }
    if (c === "$" && text[i + 1] === "{") {
      hasSubstitution = true;
      let depth: number = 1;
      i += 2;
      while (i < text.length && depth > 0) {
        const d: string = text[i] as string;
        if (d === "\\") {
          i += 2;
          continue;
        }
        if (d === "`") {
          const nested: { end: number; hasSubstitution: boolean } = scanTemplate(text, i);
          i = nested.end;
          continue;
        }
        if (d === "'" || d === '"') {
          const quote: string = d;
          i++;
          while (i < text.length) {
            const e: string = text[i] as string;
            if (e === "\\") {
              i += 2;
              continue;
            }
            i++;
            if (e === quote) {
              break;
            }
          }
          continue;
        }
        if (d === "/" && text[i + 1] === "/") {
          while (i < text.length && text[i] !== "\n") {
            i++;
          }
          continue;
        }
        if (d === "/" && text[i + 1] === "*") {
          const close: number = text.indexOf("*/", i + 2);
          i = close === -1 ? text.length : close + 2;
          continue;
        }
        if (d === "{") {
          depth++;
        } else if (d === "}") {
          depth--;
        }
        i++;
      }
      continue;
    }
    i++;
  }
  throw new Error("Unterminated template literal while scanning TypeScript test file");
}

function tokenizeTs(text: string, relativePath: string): Array<TsToken> {
  const tokens: Array<TsToken> = [];
  let i: number = 0;
  while (i < text.length) {
    const c: string = text[i] as string;
    if (c === " " || c === "\t" || c === "\n" || c === "\r" || c === "\v" || c === "\f") {
      i++;
      continue;
    }
    if (c === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") {
        i++;
      }
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      const close: number = text.indexOf("*/", i + 2);
      if (close === -1) {
        throw new Error(`${relativePath}: unterminated block comment`);
      }
      i = close + 2;
      continue;
    }
    if (c === "'" || c === '"') {
      const quote: string = c;
      let value: string = "";
      let j: number = i + 1;
      let closed: boolean = false;
      while (j < text.length) {
        const d: string = text[j] as string;
        if (d === "\\") {
          value += text.slice(j, j + 2);
          j += 2;
          continue;
        }
        if (d === quote) {
          closed = true;
          break;
        }
        if (d === "\n") {
          break;
        }
        value += d;
        j++;
      }
      if (!closed) {
        throw new Error(`${relativePath}:${lineOf(text, i)}: unterminated string literal`);
      }
      tokens.push({ kind: "string", value, start: i, end: j + 1 });
      i = j + 1;
      continue;
    }
    if (c === "`") {
      const scanned: { end: number; hasSubstitution: boolean } = scanTemplate(text, i);
      if (scanned.hasSubstitution) {
        tokens.push({ kind: "template", start: i, end: scanned.end });
      } else {
        tokens.push({
          kind: "string",
          value: text.slice(i + 1, scanned.end - 1),
          start: i,
          end: scanned.end,
        });
      }
      i = scanned.end;
      continue;
    }
    if (c === "/" && !endsOperand(tokens[tokens.length - 1])) {
      // Regex literal heuristic: `/` after a non-operand starts a regex.
      let j: number = i + 1;
      let inClass: boolean = false;
      let closed: boolean = false;
      while (j < text.length) {
        const d: string = text[j] as string;
        if (d === "\\") {
          j += 2;
          continue;
        }
        if (d === "\n") {
          break;
        }
        if (d === "[") {
          inClass = true;
        } else if (d === "]") {
          inClass = false;
        } else if (d === "/" && !inClass) {
          closed = true;
          break;
        }
        j++;
      }
      if (!closed) {
        tokens.push({ kind: "punct", value: "/", start: i, end: i + 1 });
        i++;
        continue;
      }
      let end: number = j + 1;
      while (end < text.length && /[a-z]/.test(text[end] as string)) {
        end++;
      }
      tokens.push({ kind: "regex", start: i, end });
      i = end;
      continue;
    }
    if (isIdentStart(c)) {
      let j: number = i + 1;
      while (j < text.length && isIdentPart(text[j] as string)) {
        j++;
      }
      tokens.push({ kind: "ident", value: text.slice(i, j), start: i, end: j });
      i = j;
      continue;
    }
    if (isDigit(c) || (c === "." && isDigit(text[i + 1] as string))) {
      let j: number = i + 1;
      while (j < text.length && /[\w.]/.test(text[j] as string)) {
        j++;
      }
      tokens.push({ kind: "number", start: i, end: j });
      i = j;
      continue;
    }
    const three: string = text.slice(i, i + 3);
    const two: string = text.slice(i, i + 2);
    if (["===", "!==", ">>>", "<<=", ">>=", "...", "**="].includes(three)) {
      tokens.push({ kind: "punct", value: three, start: i, end: i + 3 });
      i += 3;
      continue;
    }
    if (
      [
        "=>",
        "==",
        "!=",
        "<=",
        ">=",
        "&&",
        "||",
        "??",
        "+=",
        "-=",
        "*=",
        "/=",
        "%=",
        "++",
        "--",
        "<<",
        ">>",
        "**",
        "?.",
      ].includes(two)
    ) {
      tokens.push({ kind: "punct", value: two, start: i, end: i + 2 });
      i += 2;
      continue;
    }
    tokens.push({ kind: "punct", value: c, start: i, end: i + 1 });
    i++;
  }
  return tokens;
}

function matchForward(tokens: ReadonlyArray<TsToken>, openIndex: number): number {
  const open: TsToken = tokens[openIndex] as TsToken;
  if (open.kind !== "punct" || (open.value !== "(" && open.value !== "[" && open.value !== "{")) {
    throw new Error("matchForward called on a non-bracket token");
  }
  const closers: Record<string, string> = { "(": ")", "[": "]", "{": "}" };
  const want: string = closers[open.value] as string;
  let depth: number = 0;
  for (let k: number = openIndex; k < tokens.length; k++) {
    const token: TsToken = tokens[k] as TsToken;
    if (token.kind !== "punct") {
      continue;
    }
    if (token.value === open.value) {
      depth++;
    } else if (token.value === want) {
      depth--;
      if (depth === 0) {
        return k;
      }
    }
  }
  throw new Error("Unbalanced brackets while scanning TypeScript test file");
}

type TsCall = {
  callRoot: string;
  title: string;
  rootStart: number;
  end: number;
  bodyOpen: number | null;
  bodyClose: number | null;
};

// Parses `root[.name|(...)|[...]]*("title", ...)` starting at a root ident.
function parseTsCall(tokens: ReadonlyArray<TsToken>, rootIndex: number): TsCall | null {
  const rootToken: TsToken = tokens[rootIndex] as TsToken;
  if (rootToken.kind !== "ident" || !TEST_ROOTS.has(rootToken.value)) {
    return null;
  }
  let j: number = rootIndex + 1;
  while (j < tokens.length) {
    const token: TsToken = tokens[j] as TsToken;
    if (token.kind === "punct" && token.value === ".") {
      const name: TsToken | undefined = tokens[j + 1];
      if (name === undefined || name.kind !== "ident") {
        return null;
      }
      j += 2;
      continue;
    }
    if (token.kind === "punct" && (token.value === "(" || token.value === "[")) {
      if (token.value === "[") {
        j = matchForward(tokens, j) + 1;
        continue;
      }
      // Candidate call paren: the title paren holds a string literal as its
      // first top-level argument; anything else is a chain link (`.each(...)`).
      const close: number = matchForward(tokens, j);
      let k: number = j + 1;
      let first: TsToken | null = null;
      while (k < close) {
        const inner: TsToken = tokens[k] as TsToken;
        if (
          inner.kind === "punct" &&
          (inner.value === "(" || inner.value === "[" || inner.value === "{")
        ) {
          k = matchForward(tokens, k) + 1;
          continue;
        }
        if (
          inner.kind === "punct" &&
          (inner.value === ")" || inner.value === "]" || inner.value === "}")
        ) {
          break;
        }
        first = inner;
        break;
      }
      if (first !== null && first.kind === "string") {
        const body: { open: number; close: number } | null = findTsCallbackBody(tokens, j, close);
        let endToken: number = close;
        const semi: TsToken | undefined = tokens[close + 1];
        if (semi !== undefined && semi.kind === "punct" && semi.value === ";") {
          endToken = close + 1;
        }
        return {
          callRoot: rootToken.value,
          title: first.value,
          rootStart: rootToken.start,
          end: (tokens[endToken] as TsToken).end,
          bodyOpen: body === null ? null : body.open,
          bodyClose: body === null ? null : body.close,
        };
      }
      if (first === null) {
        return null;
      }
      // Chain link such as `.each(data)`; keep scanning past it.
      j = close + 1;
      continue;
    }
    return null;
  }
  return null;
}

// Finds an arrow (`=> {`) or `function` body at the top level of call parens.
function findTsCallbackBody(
  tokens: ReadonlyArray<TsToken>,
  openIndex: number,
  closeIndex: number,
): { open: number; close: number } | null {
  let k: number = openIndex + 1;
  while (k < closeIndex) {
    const token: TsToken = tokens[k] as TsToken;
    if (
      token.kind === "punct" &&
      (token.value === "(" || token.value === "[" || token.value === "{")
    ) {
      k = matchForward(tokens, k) + 1;
      continue;
    }
    if (token.kind === "punct" && token.value === "=>") {
      const next: TsToken | undefined = tokens[k + 1];
      if (next !== undefined && next.kind === "punct" && next.value === "{") {
        return { open: k + 1, close: matchForward(tokens, k + 1) };
      }
      return null;
    }
    if (token.kind === "ident" && token.value === "function") {
      let m: number = k + 1;
      const name: TsToken | undefined = tokens[m];
      if (name !== undefined && name.kind === "ident") {
        m++;
      }
      // Skip type parameters `<T>` when present.
      const generic: TsToken | undefined = tokens[m];
      if (generic !== undefined && generic.kind === "punct" && generic.value === "<") {
        let depth: number = 0;
        while (m < closeIndex) {
          const g: TsToken = tokens[m] as TsToken;
          if (g.kind === "punct" && g.value === "<") {
            depth++;
          } else if (g.kind === "punct" && g.value === ">") {
            depth--;
            if (depth === 0) {
              m++;
              break;
            }
          }
          m++;
        }
      }
      const params: TsToken | undefined = tokens[m];
      if (params !== undefined && params.kind === "punct" && params.value === "(") {
        m = matchForward(tokens, m) + 1;
      }
      const body: TsToken | undefined = tokens[m];
      if (body !== undefined && body.kind === "punct" && body.value === "{") {
        return { open: m, close: matchForward(tokens, m) };
      }
      return null;
    }
    k++;
  }
  return null;
}

type TsTestNode = {
  title: string;
  callRoot: string;
  scope: number;
  rootStart: number;
  end: number;
};

function tsTestNodes(text: string, relativePath: string): Array<TsTestNode> {
  const tokens: Array<TsToken> = tokenizeTs(text, relativePath);
  const calls: Array<TsCall & { rootIndex: number }> = [];
  for (let t: number = 0; t < tokens.length; t++) {
    const token: TsToken = tokens[t] as TsToken;
    if (token.kind !== "ident" || !TEST_ROOTS.has(token.value)) {
      continue;
    }
    const prev: TsToken | undefined = tokens[t - 1];
    if (prev !== undefined && prev.kind === "punct" && prev.value === ".") {
      continue;
    }
    const call: TsCall | null = parseTsCall(tokens, t);
    if (call !== null) {
      calls.push({ ...call, rootIndex: t });
    }
  }
  // Describe bodies form scopes; innermost span wins for containment.
  const describes: Array<{ open: number; close: number; order: number }> = [];
  calls.forEach((call: TsCall & { rootIndex: number }, order: number) => {
    if (call.callRoot === "describe" && call.bodyOpen !== null && call.bodyClose !== null) {
      describes.push({ open: call.bodyOpen, close: call.bodyClose, order });
    }
  });
  return calls.map((call: TsCall & { rootIndex: number }) => {
    let scope: number = -1;
    let scopeOrder: number = -1;
    describes.forEach((describe: { open: number; close: number; order: number }, order: number) => {
      if (
        describe.open < call.rootIndex &&
        call.rootIndex < describe.close &&
        order >= scopeOrder
      ) {
        scope = order;
        scopeOrder = order;
      }
    });
    return {
      title: call.title,
      callRoot: call.callRoot,
      scope,
      rootStart: call.rootStart,
      end: call.end,
    };
  });
}

function checkTsTests(
  relativePath: string,
  nodes: ReadonlyArray<TsTestNode>,
  violations: Array<Violation>,
  text: string,
): void {
  const byScope: Map<number, Array<TsTestNode>> = new Map();
  for (const node of nodes) {
    const group: Array<TsTestNode> | undefined = byScope.get(node.scope);
    if (group === undefined) {
      byScope.set(node.scope, [node]);
    } else {
      group.push(node);
    }
  }
  for (const group of byScope.values()) {
    for (let i: number = 0; i + 1 < group.length; i++) {
      const current: TsTestNode = group[i] as TsTestNode;
      const next: TsTestNode = group[i + 1] as TsTestNode;
      if (!isSortedPair(current.title, next.title)) {
        violations.push({
          file: relativePath,
          line: lineOf(text, next.rootStart),
          message: `"${next.title}" should sort before "${current.title}"`,
        });
      }
    }
  }
}

// Non-test statements stay fixed; test blocks permute among test slots.
function fixTsTests(text: string, relativePath: string): string {
  let current: string = text;
  for (let i: number = 0; i < 50; i++) {
    const nodes: Array<TsTestNode> = tsTestNodes(current, relativePath);
    const byScope: Map<number, Array<TsTestNode>> = new Map();
    for (const node of nodes) {
      const group: Array<TsTestNode> | undefined = byScope.get(node.scope);
      if (group === undefined) {
        byScope.set(node.scope, [node]);
      } else {
        group.push(node);
      }
    }
    let fixedScope: Array<TsTestNode> | null = null;
    for (const group of byScope.values()) {
      let ordered: boolean = true;
      for (let g: number = 0; g + 1 < group.length; g++) {
        if (!isSortedPair((group[g] as TsTestNode).title, (group[g + 1] as TsTestNode).title)) {
          ordered = false;
          break;
        }
      }
      if (!ordered) {
        fixedScope = group;
        break;
      }
    }
    if (fixedScope === null) {
      return current;
    }
    const group: Array<TsTestNode> = fixedScope;
    const sorted: Array<TsTestNode> = group
      .map((node: TsTestNode, index: number) => ({ node, index }))
      .sort((a, b) => {
        if (a.node.title < b.node.title) {
          return -1;
        }
        if (a.node.title > b.node.title) {
          return 1;
        }
        return a.index - b.index;
      })
      .map((entry: { node: TsTestNode; index: number }) => entry.node);
    const blocks: Array<string> = sorted.map((node: TsTestNode) =>
      current.slice(node.rootStart, node.end),
    );
    const first: TsTestNode = group[0] as TsTestNode;
    let out: string = current.slice(0, first.rootStart);
    group.forEach((node: TsTestNode, slotIndex: number) => {
      out += blocks[slotIndex] as string;
      const next: TsTestNode | undefined = group[slotIndex + 1];
      out += next === undefined ? current.slice(node.end) : current.slice(node.end, next.rootStart);
    });
    current = out;
  }
  throw new Error(`${relativePath}: test order did not converge while fixing`);
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

type RustTest = {
  name: string;
  start: number;
  end: number;
};

const rustTestAttribute: RegExp = /#\[(?:tokio::)?test\]/g;
const rustFnName: RegExp = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/g;

function isAttributeOrDocLine(line: string): boolean {
  const trimmed: string = line.trim();
  return trimmed.startsWith("#[") || trimmed.startsWith("///") || trimmed.startsWith("//!");
}

// Finds the closing `}` matching the `{` at openIndex, skipping strings,
// chars, and comments. Handles raw/byte/C string prefixes.
function findRustBlockEnd(text: string, openIndex: number): number {
  let i: number = openIndex;
  let depth: number = 0;
  let lineComment: boolean = false;
  let blockDepth: number = 0;
  let stringOpen: boolean = false;
  while (i < text.length) {
    const c: string = text[i] as string;
    const next: string = (text[i + 1] as string | undefined) ?? "";
    if (lineComment) {
      if (c === "\n") {
        lineComment = false;
      }
      i++;
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && next === "*") {
        blockDepth++;
        i += 2;
        continue;
      }
      if (c === "*" && next === "/") {
        blockDepth--;
        i += 2;
        if (blockDepth === 0) {
          continue;
        }
        continue;
      }
      i++;
      continue;
    }
    if (stringOpen) {
      if (c === "\\") {
        i += 2;
        continue;
      }
      if (c === '"') {
        stringOpen = false;
      }
      i++;
      continue;
    }
    if (c === "/" && next === "/") {
      lineComment = true;
      i += 2;
      continue;
    }
    if (c === "/" && next === "*") {
      blockDepth = 1;
      i += 2;
      continue;
    }
    if (c === "r" && (next === '"' || next === "#")) {
      let j: number = i + 1;
      let hashes: string = "";
      while (text[j] === "#") {
        hashes += "#";
        j++;
      }
      if (text[j] === '"') {
        const closing: string = `"${hashes}`;
        const endIndex: number = text.indexOf(closing, j + 1);
        if (endIndex === -1) {
          throw new Error("Unterminated raw string while scanning Rust test block");
        }
        i = endIndex + closing.length;
        continue;
      }
      i++;
      continue;
    }
    if ((c === "b" || c === "c") && next === '"') {
      i++;
      continue;
    }
    if (c === "b" && next === "r") {
      let j: number = i + 2;
      let hashes: string = "";
      while (text[j] === "#") {
        hashes += "#";
        j++;
      }
      if (text[j] === '"') {
        const closing: string = `"${hashes}`;
        const endIndex: number = text.indexOf(closing, j + 1);
        if (endIndex === -1) {
          throw new Error("Unterminated raw byte string while scanning Rust test block");
        }
        i = endIndex + closing.length;
        continue;
      }
      i++;
      continue;
    }
    if (c === '"') {
      stringOpen = true;
      i++;
      continue;
    }
    if (c === "'") {
      const charMatch: RegExpMatchArray | null = /^'(?:\\.|[^'\\])'/.exec(text.slice(i, i + 8));
      if (charMatch !== null) {
        i += charMatch[0].length;
        continue;
      }
      // Lifetime parameter, not a char literal.
      i++;
      continue;
    }
    if (c === "{") {
      depth++;
    } else if (c === "}") {
      depth--;
      if (depth === 0) {
        return i + 1;
      }
    }
    i++;
  }
  throw new Error("Unbalanced braces while scanning Rust test block");
}

function rustTestsInFile(text: string, relativePath: string): Array<RustTest> {
  const found: Array<RustTest> = [];
  rustTestAttribute.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = rustTestAttribute.exec(text)) !== null) {
    const attributeIndex: number = match.index;
    let lineStart: number = text.lastIndexOf("\n", attributeIndex - 1) + 1;
    // Absorb stacked attributes/doc comments directly above (no blank line).
    let cursor: number = lineStart;
    while (cursor > 0) {
      const prevLineEnd: number = cursor - 1;
      const prevLineStart: number = text.lastIndexOf("\n", prevLineEnd - 1) + 1;
      const prevLine: string = text.slice(prevLineStart, prevLineEnd);
      if (prevLine.trim() === "" || !isAttributeOrDocLine(prevLine)) {
        break;
      }
      cursor = prevLineStart;
    }
    lineStart = cursor;
    rustFnName.lastIndex = rustTestAttribute.lastIndex;
    const fnMatch: RegExpExecArray | null = rustFnName.exec(text);
    if (fnMatch === null || fnMatch[1] === undefined) {
      throw new Error(`${relativePath}:${lineOf(text, attributeIndex)}: #[test] without a fn body`);
    }
    const openBrace: number = text.indexOf("{", rustFnName.lastIndex);
    if (openBrace === -1) {
      throw new Error(
        `${relativePath}:${lineOf(text, attributeIndex)}: test fn without a body block`,
      );
    }
    const end: number = findRustBlockEnd(text, openBrace);
    found.push({ name: fnMatch[1], start: lineStart, end });
    rustTestAttribute.lastIndex = end;
  }
  return found;
}

function checkRustFile(relativePath: string, text: string, violations: Array<Violation>): void {
  const tests: Array<RustTest> = rustTestsInFile(text, relativePath);
  for (let i: number = 0; i + 1 < tests.length; i++) {
    const current: RustTest = tests[i] as RustTest;
    const next: RustTest = tests[i + 1] as RustTest;
    if (!isSortedPair(current.name, next.name)) {
      violations.push({
        file: relativePath,
        line: lineOf(text, next.start),
        message: `fn ${next.name} should sort before fn ${current.name}`,
      });
    }
  }
}

// Non-test helper items stay fixed; test blocks permute among test slots.
function fixRustFile(text: string, relativePath: string): string {
  const tests: Array<RustTest> = rustTestsInFile(text, relativePath);
  const sorted: Array<RustTest> = tests
    .map((test: RustTest, index: number) => ({ test, index }))
    .sort((a, b) => {
      if (a.test.name < b.test.name) {
        return -1;
      }
      if (a.test.name > b.test.name) {
        return 1;
      }
      return a.index - b.index;
    })
    .map((entry: { test: RustTest; index: number }) => entry.test);
  if (sorted.every((test: RustTest, index: number) => test === tests[index])) {
    return text;
  }
  const blocks: Array<string> = sorted.map((test: RustTest) =>
    text.slice(test.start, test.end).trim(),
  );
  const first: RustTest = tests[0] as RustTest;
  let out: string = text.slice(0, first.start);
  tests.forEach((test: RustTest, slotIndex: number) => {
    out += blocks[slotIndex] as string;
    const next: RustTest | undefined = tests[slotIndex + 1];
    out += next === undefined ? text.slice(test.end) : text.slice(test.end, next.start);
  });
  // Confirm the fix parses to a sorted order.
  const violations: Array<Violation> = [];
  checkRustFile(relativePath, out, violations);
  if (violations.length > 0) {
    throw new Error(`${relativePath}: test order did not converge while fixing`);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

async function listFilesRecursive(dir: string, suffix: string): Promise<Array<string>> {
  const found: Array<string> = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full: string = join(entry.parentPath, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await listFilesRecursive(full, suffix)));
    } else if (entry.name.endsWith(suffix)) {
      found.push(full);
    }
  }
  return found.sort();
}

async function main(): Promise<void> {
  const mode: Mode = parseMode(process.argv.slice(2));
  const tsFiles: Array<string> = (await readdir(tsTestDir))
    .filter((name: string) => name.endsWith(".test.ts"))
    .sort()
    .map((name: string) => join(tsTestDir, name));
  const rustFiles: Array<string> = await listFilesRecursive(rustTestDir, ".rs");

  if (mode === "fix") {
    let fixed: number = 0;
    for (const full of tsFiles) {
      const relativePath: string = full.slice(root.length + 1);
      const text: string = await readFile(full, "utf8");
      const next: string = fixTsTests(text, relativePath);
      if (next !== text) {
        await writeFile(full, next);
        fixed++;
        console.log(`sorted ${relativePath}`);
      }
    }
    for (const full of rustFiles) {
      const relativePath: string = full.slice(root.length + 1);
      const text: string = await readFile(full, "utf8");
      const next: string = fixRustFile(text, relativePath);
      if (next !== text) {
        await writeFile(full, next);
        fixed++;
        console.log(`sorted ${relativePath}`);
      }
    }
    console.log(fixed === 0 ? "test order: already sorted" : `test order: sorted ${fixed} file(s)`);
    return;
  }

  const violations: Array<Violation> = [];
  for (const full of tsFiles) {
    const relativePath: string = full.slice(root.length + 1);
    const text: string = await readFile(full, "utf8");
    checkTsTests(relativePath, tsTestNodes(text, relativePath), violations, text);
  }
  for (const full of rustFiles) {
    const relativePath: string = full.slice(root.length + 1);
    checkRustFile(relativePath, await readFile(full, "utf8"), violations);
  }
  if (violations.length > 0) {
    violations.sort((a: Violation, b: Violation) =>
      a.file.localeCompare(b.file) !== 0 ? a.file.localeCompare(b.file) : a.line - b.line,
    );
    for (const violation of violations) {
      console.error(`${violation.file}:${violation.line}: ${violation.message}`);
    }
    console.error(
      `test order: ${violations.length} violation(s). Run \`bun run lint:test-order:fix\` to sort.`,
    );
    process.exit(1);
  }
  console.log("test order: sorted");
}

await main();

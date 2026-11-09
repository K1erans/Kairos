import test from "node:test";
import assert from "node:assert/strict";
import "../lib/selector.js";

const { build, isStableName } = globalThis.KairosSelector;

// Minimal DOM stand-in: just what the builder reads.
function el(tag, { id = "", classes = [], attrs = {} } = {}, children = []) {
  const node = {
    tagName: tag.toUpperCase(),
    id,
    classList: classes,
    children,
    parentElement: null,
    getAttribute: (name) => (name in attrs ? attrs[name] : null),
  };
  for (const c of children) c.parentElement = node;
  return node;
}

// CSS string unescaping, as the browser's selector parser does it.
const cssUnescape = (s) => s.replace(/\\([0-9a-fA-F]{1,6}) ?|\\(.)/g, (_, hex, ch) => (hex ? String.fromCodePoint(parseInt(hex, 16)) : ch));

// querySelectorAll for the selector shapes the builder produces, against a tree.
function matcher(root) {
  const all = [];
  (function walk(n) {
    all.push(n);
    n.children.forEach(walk);
  })(root);
  const matchesSimple = (n, simple) => {
    const m = /^(#[^.:\[]+|[a-z0-9-]+)?((?:\.[\w-]+)*)(\[data-testid="((?:[^"\\]|\\.)*)"\])?(:nth-of-type\((\d+)\))?$/.exec(simple);
    if (!m) throw new Error("unexpected selector part: " + simple);
    if (m[1]) {
      if (m[1].startsWith("#")) {
        if (n.id !== m[1].slice(1)) return false;
      } else if (n.tagName.toLowerCase() !== m[1]) return false;
    }
    for (const c of m[2].split(".").filter(Boolean)) if (!n.classList.includes(c)) return false;
    if (m[3] && n.getAttribute("data-testid") !== cssUnescape(m[4])) return false;
    if (m[6]) {
      const same = n.parentElement.children.filter((s) => s.tagName === n.tagName);
      if (same.indexOf(n) + 1 !== Number(m[6])) return false;
    }
    return true;
  };
  return (selector) => {
    const parts = selector.split(" > ");
    return all.filter((n) => {
      let node = n;
      for (let i = parts.length - 1; i >= 0; i--) {
        if (!node || !matchesSimple(node, parts[i])) return false;
        node = node.parentElement;
      }
      return true;
    });
  };
}

test("stable names are kept, generated ones are not", () => {
  for (const good of ["sidebar", "related-videos", "nav_main"]) assert.ok(isStableName(good), good);
  for (const bad of ["css-1q2w3e4", "sc-bdVaJa", "a1b2c3d4e5", "item-12345", "is-active", "", "x".repeat(50)]) assert.ok(!isStableName(bad), bad);
});

test("a unique stable id wins", () => {
  const target = el("div", { id: "related" });
  const root = el("body", {}, [el("div", {}, [target])]);
  assert.equal(build(target, matcher(root)), "#related");
});

test("generated ids are skipped in favour of classes", () => {
  const target = el("aside", { id: "ember12345", classes: ["sidebar"] });
  const root = el("body", {}, [el("main", {}, [target])]);
  assert.equal(build(target, matcher(root)), "aside.sidebar");
});

test("data-testid is used when present", () => {
  const target = el("div", { attrs: { "data-testid": "sidebarColumn" } });
  const root = el("body", {}, [target, el("div")]);
  assert.equal(build(target, matcher(root)), 'div[data-testid="sidebarColumn"]');
});

test("siblings with the same class are told apart by position", () => {
  const a = el("section", { classes: ["card"] });
  const b = el("section", { classes: ["card"] });
  const root = el("body", {}, [el("div", { id: "feed" }, [a, b])]);
  const q = matcher(root);
  const count = (x) => q(x).length;
  const sel = build(b, q);
  assert.equal(count(sel), 1);
  assert.match(sel, /nth-of-type\(2\)/);
});

test("class-less elements climb to an ancestor id and stay unique", () => {
  const target = el("div");
  const other = el("div");
  const root = el("body", {}, [el("div", { id: "page" }, [el("ul", {}, [el("li", {}, [other]), el("li", {}, [target])])]), el("div")]);
  const q = matcher(root);
  const count = (x) => q(x).length;
  const sel = build(target, q);
  assert.equal(count(sel), 1, sel);
  assert.ok(!/html|body/.test(sel));
});

test("returns an empty string for nothing", () => {
  assert.equal(build(null, () => []), "");
});

test("a selector that still matches several elements after climbing is refused", () => {
  // Eight nested levels of identical, class-less wrappers, repeated twice: no prefix is unique.
  const chain = () => { let n = el("span"); for (let i = 0; i < 8; i++) n = el("div", {}, [n]); return n; };
  const a = chain();
  const b = chain();
  const root = el("body", {}, [el("section", {}, [a]), el("section", {}, [b])]);
  let target = a;
  while (target.children.length) target = target.children[0];
  const q = matcher(root);
  const count = (x) => q(x).length;
  const sel = build(target, q);
  assert.ok(sel === "" || count(sel) === 1, `got ${sel} matching ${count(sel)}`);
});

test("data-testid values are escaped", () => {
  const target = el("div", { attrs: { "data-testid": "col\\umn" } });
  const root = el("body", {}, [target, el("div")]);
  const q = matcher(root);
  const count = (x) => q(x).length;
  const sel = build(target, q);
  assert.equal(count(sel), 1, sel);
});

test("an element inside a shadow root is pinned through its host", () => {
  const inner = el("button", { classes: ["like"] });
  const host = el("x-card", { id: "card-a" });
  const shadow = { host, children: [inner] };
  inner.getRootNode = () => shadow;
  const root = el("body", {}, [host, el("x-card", { id: "card-b" })]);
  const q = matcher(root);
  const count = (x) => q(x).length;
  const sel = build(inner, q);
  assert.equal(count(sel), 1, sel);
});

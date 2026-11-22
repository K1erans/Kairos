import test from "node:test";
import assert from "node:assert/strict";
import { sanitizeState } from "../lib/match.js";
import {
  JEV_CACHE_MS,
  PRESETS,
  buildDnrRules,
  canOverride,
  cssForSelectors,
  effectiveState,
  firstWords,
  isBalancedSelector,
  isSafeSelector,
  jevCacheKey,
  jevSkipReason,
  nextExpiry,
  originalUrlFromHash,
  pruneJevCache,
  selectorsForHost,
} from "../lib/rules.js";

const BLOCKED = "chrome-extension://abc/blocked.html";
const make = (over = {}, rules = {}) =>
  sanitizeState({ mode: "focus", activeGroupId: "g1", rules: { blockSites: ["youtube.com", "https://www.reddit.com/"], ...rules }, ...over });

test("no rules outside focus and strict", () => {
  for (const mode of ["off", "break"]) assert.deepEqual(buildDnrRules(make({ mode }), { blockedPageUrl: BLOCKED }), []);
  assert.equal(buildDnrRules(make({ mode: "strict" }), { blockedPageUrl: BLOCKED }).length, 2);
});

test("frames of a blocked site are blocked too, and allows cover frames as well", () => {
  const rules = buildDnrRules(make({}, { allowSites: ["music.youtube.com"], temporaryAllows: { "reddit.com": 5000 } }), { blockedPageUrl: BLOCKED, now: 1000 });
  const frame = rules.find((r) => r.condition.resourceTypes.includes("sub_frame") && r.action.type !== "allow");
  assert.equal(frame.action.type, "block", "not redirected: the block page refuses to work in a frame");
  assert.deepEqual(frame.condition.requestDomains, ["reddit.com", "youtube.com"]);
  for (const allow of rules.filter((r) => r.action.type === "allow")) {
    assert.deepEqual(allow.condition.resourceTypes, ["main_frame", "sub_frame"]);
    assert.ok(allow.priority > frame.priority);
  }
});

test("block rule redirects main frames to the block page with the URL in the hash", () => {
  const [rule] = buildDnrRules(make(), { blockedPageUrl: BLOCKED });
  assert.equal(rule.action.type, "redirect");
  assert.equal(rule.action.redirect.regexSubstitution, `${BLOCKED}#\\1`);
  assert.deepEqual(rule.condition.requestDomains, ["reddit.com", "youtube.com"]);
  assert.deepEqual(rule.condition.resourceTypes, ["main_frame"]);
  // The regex must capture the whole URL as group 1.
  const m = new RegExp(rule.condition.regexFilter).exec("https://www.youtube.com/watch?v=1#t=3");
  assert.equal(m[1], "https://www.youtube.com/watch?v=1#t=3");
});

test("allow list and live temporary allows get higher priority than the block", () => {
  const rules = buildDnrRules(
    make({}, { allowSites: ["music.youtube.com"], temporaryAllows: { "reddit.com": 5000, "x.com": 10 } }),
    { blockedPageUrl: BLOCKED, now: 1000 },
  );
  assert.equal(rules.length, 4);
  const [block, , allow, temp] = rules;
  assert.ok(allow.priority > block.priority && temp.priority > allow.priority);
  assert.equal(allow.action.type, "allow");
  assert.deepEqual(allow.condition.requestDomains, ["music.youtube.com"]);
  assert.deepEqual(temp.condition.requestDomains, ["reddit.com"]); // x.com expired
  assert.equal(new Set(rules.map((r) => r.id)).size, 4);
});

test("no rule is emitted with an empty domain list", () => {
  assert.deepEqual(buildDnrRules(make({}, { blockSites: [], allowSites: ["a.com"] }), { blockedPageUrl: BLOCKED }), []);
  assert.deepEqual(buildDnrRules(make({}, { blockSites: ["???"] }), { blockedPageUrl: BLOCKED }), []);
});

test("effectiveState uses manual mode only while disconnected and merges local allows", () => {
  const base = make({}, { temporaryAllows: { "a.com": 5000, "old.com": 1 } });
  const away = effectiveState(base, { connected: false, manualMode: "off", localAllows: { "b.com": 9000 }, now: 1000 });
  assert.equal(away.mode, "off");
  const awayFocus = effectiveState(make({ mode: "break" }, base.rules), { connected: false, manualMode: "focus", localAllows: { "b.com": 9000 }, now: 1000 });
  assert.equal(awayFocus.mode, "focus");
  assert.deepEqual(awayFocus.rules.temporaryAllows, { "a.com": 5000, "b.com": 9000 });
  const here = effectiveState(base, { connected: true, manualMode: "off", localAllows: {}, now: 1000 });
  assert.equal(here.mode, "focus");
  assert.equal(base.rules.temporaryAllows["old.com"], 1, "input state is not mutated");
});

test("local allows count in focus only: a grant from focus does not carry into strict", () => {
  const strict = make({ mode: "strict" });
  const here = effectiveState(strict, { connected: true, manualMode: null, localAllows: { "youtube.com": 9000 }, now: 1000 });
  assert.deepEqual(here.rules.temporaryAllows, {});
  const locked = make({ mode: "focus", strictUntil: 50_000 });
  const away = effectiveState(locked, { connected: false, manualMode: "focus", localAllows: { "youtube.com": 9000 }, now: 1000 });
  assert.equal(away.mode, "strict");
  assert.deepEqual(away.rules.temporaryAllows, {});
});

test("nextExpiry picks the soonest future expiry", () => {
  assert.equal(nextExpiry({ a: 500, b: 3000, c: 2000 }, 1000), 2000);
  assert.equal(nextExpiry({ a: 500 }, 1000), null);
});

test("selectorsForHost merges presets, group rules and local rules per the mode table", () => {
  const s = make({}, { hiddenElements: { "youtube.com": ["#comments"], "other.com": ["#x"] } });
  const got = selectorsForHost("www.youtube.com", s, { "youtube.com": [".mine", "#comments"] });
  assert.ok(got.includes("#related") && got.includes("#comments") && got.includes(".mine"));
  assert.ok(!got.includes("#x"));
  assert.equal(got.filter((x) => x === "#comments").length, 1);
  assert.ok(selectorsForHost("youtube.com", make({ mode: "break" })).length > 0, "break keeps element rules");
  assert.deepEqual(selectorsForHost("youtube.com", make({ mode: "off" })), []);
  assert.deepEqual(selectorsForHost("example.org", s), []);
});

test("presets cover the four sites in the spec and are all safe selectors", () => {
  for (const site of ["youtube.com", "x.com", "reddit.com", "linkedin.com"]) assert.ok(PRESETS[site].length > 0, site);
  for (const list of Object.values(PRESETS)) for (const sel of list) assert.ok(isSafeSelector(sel), sel);
});

test("unsafe selectors are dropped and CSS is one rule per selector", () => {
  assert.ok(!isSafeSelector("a{} body{display:none}"));
  assert.ok(!isSafeSelector(""));
  const s = make({}, { hiddenElements: { "a.com": ["#ok", "x } body { color:red"] } });
  assert.deepEqual(selectorsForHost("a.com", s), ["#ok"]);
  assert.equal(cssForSelectors(["#a", ".b"]), "#a{display:none!important}\n.b{display:none!important}");
});

test("a selector with an unclosed bracket, paren or quote is refused, so it cannot swallow the rules after it", () => {
  for (const bad of ["a[href", "div:not(.x", 'a[title="x]', "a[title='x]", "div)", "a]", "a\\", 'a[title="x\\"]']) {
    assert.ok(!isSafeSelector(bad), bad);
  }
  for (const good of ["a[href]", "div:not(.x)", 'a[title="x]"]', "a[title='(']", "#a\\:b", 'a[title="x\\"y"]', "ytd-guide-entry-renderer:has(a[title='Shorts'])"]) {
    assert.ok(isSafeSelector(good), good);
    assert.ok(isBalancedSelector(good), good);
  }
  const s = make({}, { hiddenElements: { "a.com": ["#one", "a[href", "#two"] } });
  assert.deepEqual(selectorsForHost("a.com", s), ["#one", "#two"]);
});

test("firstWords caps the Jev excerpt at 500 words", () => {
  const text = Array.from({ length: 800 }, (_, i) => `w${i}`).join("  \n ");
  const out = firstWords(text).split(" ");
  assert.equal(out.length, 500);
  assert.equal(out[499], "w499");
  assert.equal(firstWords(""), "");
});

test("Jev cache is keyed per URL (without hash) and group, and expires after 24h", () => {
  assert.equal(jevCacheKey("https://a.com/p?q=1#frag", make()), jevCacheKey("https://a.com/p?q=1", make()));
  assert.notEqual(jevCacheKey("https://a.com/p", make()), jevCacheKey("https://a.com/p", make({ activeGroupId: "g2" })));
  const now = 10 * JEV_CACHE_MS;
  const pruned = pruneJevCache({ fresh: { at: now - 1000, block: true }, stale: { at: now - JEV_CACHE_MS - 1, block: true }, junk: null }, now);
  assert.deepEqual(Object.keys(pruned), ["fresh"]);
});

test("jevSkipReason: only unlisted http(s) pages in focus/strict with Jev on, never incognito or never-analyse", () => {
  const s = make({}, { jevEnabled: true, neverAnalyse: ["mail.google.com"], allowSites: ["docs.google.com"] });
  const ctx = { connected: true, incognito: false, now: 1000 };
  assert.equal(jevSkipReason("https://example.org/a", s, ctx), null);
  assert.equal(jevSkipReason("https://example.org/a", make({ mode: "strict" }, { jevEnabled: true }), ctx), null);
  assert.equal(jevSkipReason("https://example.org/a", s, { ...ctx, incognito: true }), "incognito");
  assert.equal(jevSkipReason("https://example.org/a", s, { ...ctx, connected: false }), "disconnected");
  assert.equal(jevSkipReason("https://example.org/a", make({ mode: "break" }, { jevEnabled: true }), ctx), "mode");
  assert.equal(jevSkipReason("https://example.org/a", make(), ctx), "disabled");
  assert.equal(jevSkipReason("chrome://settings", s, ctx), "scheme");
  assert.equal(jevSkipReason("https://mail.google.com/mail", s, ctx), "never-analyse");
  assert.equal(jevSkipReason("https://docs.google.com/d/1", s, ctx), "listed");
  assert.equal(jevSkipReason("https://youtube.com/", s, ctx), "listed");
});

test("originalUrlFromHash keeps everything after the first # and rejects non-web URLs", () => {
  assert.equal(originalUrlFromHash("#https://a.com/p?q=1#frag"), "https://a.com/p?q=1#frag");
  assert.equal(originalUrlFromHash("#javascript:alert(1)"), "");
  assert.equal(originalUrlFromHash(""), "");
});

test("overrides only exist in focus mode", () => {
  assert.ok(canOverride(make()));
  assert.ok(!canOverride(make({ mode: "strict" })));
  assert.ok(!canOverride(make({ mode: "break" })));
});

test("Unicode block entries reach the DNR rule as punycode", () => {
  const [rule] = buildDnrRules(make({}, { blockSites: ["münchen.de"] }), { blockedPageUrl: BLOCKED });
  assert.deepEqual(rule.condition.requestDomains, ["xn--mnchen-3ya.de"]);
});

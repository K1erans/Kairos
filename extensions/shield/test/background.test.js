import test from "node:test";
import assert from "node:assert/strict";
import { fakeApp, startShield, tick } from "./harness.js";

const PAGE = { type: "jev.page", url: "https://blog.example/post", title: "Post", description: "", excerpt: "words" };
const focus = (over = {}) => ({ mode: "focus", activeGroupId: "g1", currentTask: null, rules: { jevEnabled: true, jevThreshold: 0.7, blockSites: ["youtube.com"] }, ...over });

function jevApp(state, verdicts) {
  return fakeApp(() => state.current, (msg, reply) => {
    if (msg.type === "jev.check") reply({ type: "jev.result", id: msg.id, ...verdicts.shift() });
  });
}

test("a Jev verdict made with no task is not reused once a task is set or the threshold changes", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: jevApp(state, [
    { choice: "relevant", block: false },
    { choice: "distracting", block: true },
    { choice: "relevant", block: false },
  ]) });
  t.after(() => h.dispose());
  assert.equal((await h.send(PAGE)).block, false);
  await h.push(focus({ currentTask: { id: "t1", title: "Write report" } }));
  assert.equal((await h.send(PAGE)).block, true, "asked again for the new task");
  await h.push(focus({ currentTask: { id: "t1", title: "Write report" }, rules: { jevEnabled: true, jevThreshold: 0.9 } }));
  assert.equal((await h.send(PAGE)).block, false, "asked again for the new threshold");
  assert.equal(h.sent("jev.check").length, 3);
  await h.send(PAGE);
  assert.equal(h.sent("jev.check").length, 3, "the same task and threshold reuse the cached verdict");
});

test("an unsure verdict is not cached", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: jevApp(state, [{ choice: "unsure", block: false }, { choice: "distracting", block: true }]) });
  t.after(() => h.dispose());
  await h.send(PAGE);
  assert.equal((await h.send(PAGE)).block, true);
  assert.equal(h.sent("jev.check").length, 2);
});

test("Show on a hidden element asks the app to forget it and unhides it at once", async (t) => {
  const state = { current: focus({ rules: { hiddenElements: {} } }) };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.send({ type: "picker.picked", host: "news.example", selector: "#promo" });
  await h.push(focus({ rules: { hiddenElements: { "news.example": ["#promo"] } } })); // the app stored it too
  await h.send({ type: "elements.remove", host: "news.example", selector: "#promo" });
  assert.deepEqual(h.sent("rule.removeElement"), [{ type: "rule.removeElement", host: "news.example", selector: "#promo" }]);
  const info = await h.send({ type: "page.query", url: "https://news.example/" });
  assert.ok(!info.selectors.includes("#promo"), "hidden right away, before the app pushes a new state");
});

test("a locked strict phase keeps hidden elements hidden", async (t) => {
  const until = Date.now() + 25 * 60 * 1000;
  const strict = (over = {}) => focus({ mode: "strict", strictUntil: until, rules: { hiddenElements: { "news.example": ["#promo"] } }, ...over });
  const state = { current: strict() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.send({ type: "picker.picked", host: "news.example", selector: "#promo" });
  const reply = await h.send({ type: "elements.remove", host: "news.example", selector: "#promo" });
  assert.equal(reply.ok, false);
  assert.equal(reply.lockedUntil, until);
  assert.match(reply.message, /Strict focus until .*: you can block more/);
  assert.equal(h.sent("rule.removeElement").length, 0, "the app is not asked");
  const info = await h.send({ type: "page.query", url: "https://news.example/" });
  assert.ok(info.selectors.includes("#promo"), "still hidden");
  assert.equal((await h.send({ type: "popup.get", url: "https://news.example/" })).rulesLockedUntil, until);
  assert.equal((await h.send({ type: "picker.picked", host: "news.example", selector: "#more" })).ok, true, "hiding more is fine");
});

test("an element picked while the app is away reaches it after it reconnects", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.drop();
  await h.send({ type: "picker.picked", host: "news.example", selector: "#promo" });
  assert.equal(h.sent("rule.addElement").length, 0);
  await h.reconnect(fakeApp(() => state.current));
  assert.deepEqual(h.sent("rule.addElement"), [{ type: "rule.addElement", host: "news.example", selector: "#promo" }]);
});

test("the page is told when Jev stops applying, so an overlay can be cleared", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "page.query", url: PAGE.url })).jev, true);
  await h.drop();
  assert.equal((await h.send({ type: "page.query", url: PAGE.url })).jev, false);
});

test("opening the block page for a site that is not blocked logs nothing", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.send({ type: "blocked.shown", url: "https://evil.example/landing" }, { url: "chrome-extension://shield/blocked.html" });
  assert.equal(h.sent("blockEvent").length, 0);
  await h.send({ type: "blocked.shown", url: "https://www.youtube.com/watch?v=1" }, { url: "chrome-extension://shield/blocked.html" });
  assert.equal(h.sent("blockEvent").length, 1);
});

// ---- strict lock ------------------------------------------------------------------------------

const lockedState = (strictUntil, over = {}) => ({ mode: "strict", activeGroupId: "g1", strictUntil, rules: { jevEnabled: true, blockSites: ["youtube.com"] }, ...over });
const YT = "https://www.youtube.com/watch?v=1";

test("a locked strict phase keeps being enforced when the app disconnects, and the popup cannot switch it off", async (t) => {
  const until = Date.now() + 10 * 60_000;
  const state = { current: lockedState(until) };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.drop();
  const refused = await h.send({ type: "popup.setMode", mode: "off" });
  assert.equal(refused.ok, false);
  assert.equal(refused.lockedUntil, until);
  const popup = await h.send({ type: "popup.get", url: YT });
  assert.equal(popup.state.mode, "strict");
  assert.equal(popup.lockedUntil, until);
  assert.equal(h.dnr.rules.length > 0, true, "blocking rules stay installed");
  assert.equal((await h.send({ type: "page.query", url: YT })).verdict, "blocked");
});

test("a manual mode saved before the lock began does not override it", async (t) => {
  const until = Date.now() + 10 * 60_000;
  const h = await startShield({ storage: { local: { state: lockedState(until), manualMode: "off" } } });
  t.after(() => h.dispose());
  const popup = await h.send({ type: "popup.get", url: YT });
  assert.equal(popup.state.mode, "strict");
  assert.equal((await h.send({ type: "page.query", url: YT })).verdict, "blocked");
});

test("the lock survives a worker restart while the app is away", async (t) => {
  const until = Date.now() + 10 * 60_000;
  const h = await startShield({ storage: { local: { state: lockedState(until) } } });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "popup.setMode", mode: "break" })).ok, false);
  assert.equal((await h.send({ type: "page.query", url: YT })).verdict, "blocked");
});

test("a lock reported with a looser last mode still enforces strict while disconnected", async (t) => {
  const until = Date.now() + 10 * 60_000;
  const h = await startShield({ storage: { local: { state: lockedState(until, { mode: "focus" }) } } });
  t.after(() => h.dispose());
  const info = await h.send({ type: "page.query", url: YT });
  assert.equal(info.mode, "strict");
  assert.equal(info.canOverride, false);
});

test("once the lock time has passed the popup may switch modes again", async (t) => {
  const h = await startShield({ storage: { local: { state: lockedState(Date.now() - 1000) } } });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "popup.get", url: YT })).lockedUntil, null);
  assert.equal((await h.send({ type: "popup.setMode", mode: "off" })).ok, true);
  assert.equal((await h.send({ type: "page.query", url: YT })).verdict, "off");
});

test("the app clearing strictUntil releases the lock after a reconnect", async (t) => {
  const state = { current: lockedState(Date.now() + 10 * 60_000) };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.drop();
  state.current = lockedState(null, { mode: "focus" });
  await h.reconnect(fakeApp(() => state.current));
  await h.drop();
  assert.equal((await h.send({ type: "popup.setMode", mode: "off" })).ok, true);
});

// ---- block / allow this site ------------------------------------------------------------------

const BLOG = "https://www.blog.example/post";

test("Block this site applies at once and tells the app", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "page.query", url: BLOG })).verdict, "unlisted");
  const reply = await h.send({ type: "site.add", host: "www.blog.example", list: "block" });
  assert.deepEqual([reply.ok, reply.site], [true, "blog.example"]);
  assert.equal((await h.send({ type: "page.query", url: BLOG })).verdict, "blocked");
  assert.deepEqual(h.sent("rule.addSite"), [{ type: "rule.addSite", site: "blog.example", list: "block" }]);
  assert.ok(h.dnr.rules.length > 0, "the redirect rule covers it");
});

test("Always allow moves a blocked site to the allow list", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "page.query", url: "https://www.youtube.com/" })).verdict, "blocked");
  await h.send({ type: "site.add", host: "youtube.com", list: "allow" });
  assert.equal((await h.send({ type: "page.query", url: "https://www.youtube.com/" })).verdict, "allowed");
  assert.deepEqual(h.sent("rule.addSite"), [{ type: "rule.addSite", site: "youtube.com", list: "allow" }]);
});

test("blocking a site ends a temporary allow on it", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.push(focus({ rules: { temporaryAllows: { "blog.example": Date.now() + 60_000 } } }));
  assert.equal((await h.send({ type: "page.query", url: BLOG })).verdict, "allowed");
  await h.send({ type: "site.add", host: "blog.example", list: "block" });
  assert.equal((await h.send({ type: "page.query", url: BLOG })).verdict, "blocked");
});

test("a site chosen while the app is away is sent when it returns", async (t) => {
  const state = { current: focus() };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.drop();
  await h.send({ type: "site.add", host: "blog.example", list: "block" });
  assert.equal(h.sent("rule.addSite").length, 0);
  assert.equal((await h.send({ type: "page.query", url: BLOG })).verdict, "blocked", "enforced meanwhile");
  await h.reconnect(fakeApp(() => state.current));
  assert.deepEqual(h.sent("rule.addSite"), [{ type: "rule.addSite", site: "blog.example", list: "block" }]);
});

test("strict mode does not let a site be allowed for good", async (t) => {
  const state = { current: focus({ mode: "strict" }) };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  const reply = await h.send({ type: "site.add", host: "youtube.com", list: "allow" });
  assert.equal(reply.ok, false);
  assert.match(reply.message, /strict/i);
  assert.equal(h.sent("rule.addSite").length, 0);
  assert.equal((await h.send({ type: "site.add", host: "blog.example", list: "block" })).ok, true, "blocking more is fine");
});

test("a bad host or list is refused", async (t) => {
  const h = await startShield({ app: fakeApp(() => focus()) });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "site.add", host: "", list: "block" })).ok, false);
  assert.equal((await h.send({ type: "site.add", host: "blog.example", list: "both" })).ok, false);
  assert.equal(h.sent("rule.addSite").length, 0);
});

// ---- override length --------------------------------------------------------------------------

test("page info carries the app's override length, defaulting to 5 minutes", async (t) => {
  const state = { current: focus({ overrideMinutes: 12 }) };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  assert.equal((await h.send({ type: "page.query", url: BLOG })).overrideMinutes, 12);
  await h.push(focus());
  assert.equal((await h.send({ type: "page.query", url: BLOG })).overrideMinutes, 5);
  await h.push(focus({ overrideMinutes: -3 }));
  assert.equal((await h.send({ type: "page.query", url: BLOG })).overrideMinutes, 5, "nonsense falls back");
});

test("an override granted while the app is away lasts the length the app last reported", async (t) => {
  const state = { current: focus({ overrideMinutes: 10 }) };
  const h = await startShield({ app: fakeApp(() => state.current) });
  t.after(() => h.dispose());
  await h.drop();
  const before = Date.now();
  const reply = await h.send({ type: "override", target: "youtube.com", reason: "docs" });
  assert.equal(reply.allowed, true);
  assert.ok(Math.abs(reply.until - (before + 10 * 60_000)) < 5000, `until ${reply.until - before}`);
});

// ---- connection messages ----------------------------------------------------------------------

test("the popup says Kairos Desktop is not running when the host reports it", async (t) => {
  const app = (msg, reply) => { if (msg.type === "hello") reply({ type: "error", message: "Kairos Desktop is not running" }); };
  const h = await startShield({ app });
  t.after(() => h.dispose());
  await h.drop();
  const popup = await h.send({ type: "popup.get", url: BLOG });
  assert.equal(popup.connected, false);
  assert.match(popup.connection.text, /not running/);
  assert.match(popup.connection.hint, /Open Kairos Desktop/);
});

test("the popup says the bridge is not installed when Chrome cannot find the host", async (t) => {
  const h = await startShield({ app: fakeApp(() => focus()) });
  t.after(() => h.dispose());
  globalThis.chrome.runtime.lastError = { message: "Specified native messaging host not found." };
  await h.drop();
  globalThis.chrome.runtime.lastError = undefined;
  assert.match((await h.send({ type: "popup.get", url: BLOG })).connection.text, /bridge is not installed/);
});

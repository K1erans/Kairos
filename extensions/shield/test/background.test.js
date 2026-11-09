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

import test from "node:test";
import assert from "node:assert/strict";
import { articleTab, fakeApp, html, startRead, tick } from "./harness.js";

const YT = "https://www.youtube.com/watch?v=1";
const FOCUS = { mode: "focus", rules: { blockSites: ["youtube.com"], neverAnalyse: ["mail.example"] }, cloudVoice: { provider: "elevenlabs", name: "ElevenLabs" } };
const BLOCKED = /Kairos Shield blocks this page/;

async function startOn(url, opts) {
  const h = await startRead({ tabs: [articleTab(1, url, ["One.", "Two."])], windows: [{ id: 1, focused: true }], ...opts });
  return h;
}

test("a blocked site stays blocked after Kairos Desktop disconnects", async (t) => {
  const h = await startOn(YT, { app: fakeApp(FOCUS) });
  t.after(() => h.dispose());
  await h.dropPort();
  await h.cmd("start");
  await tick(5);
  assert.match(h.lastPlayer().notice, BLOCKED);
  assert.equal(h.spoken.length, 0);
  assert.equal(h.lastPlayer().mode, "focus");
});

test("the last state is saved and used when the worker starts with the app closed", async (t) => {
  const first = await startOn(YT, { app: fakeApp(FOCUS) });
  first.dispose();
  assert.equal(first.local.state.mode, "focus");
  const h = await startOn(YT, { storage: { local: { state: first.local.state } } });
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(5);
  assert.match(h.lastPlayer().notice, BLOCKED);
  assert.equal(h.spoken.length, 0);
});

test("a fetched link to a blocked site is refused while the app is closed", async (t) => {
  let fetched = 0;
  const h = await startOn("https://news.example/a", {
    storage: { local: { state: FOCUS } },
    fetch: async () => { fetched++; return html("One. Two."); },
  });
  t.after(() => h.dispose());
  await h.send({ type: "queue.add", url: YT, title: "Video" });
  await h.send({ type: "queue.play", index: 0 });
  await tick(10);
  assert.equal(fetched, 0);
});

test("a locked strict phase the app reported keeps blocking while it is away", async (t) => {
  const state = { mode: "break", strictUntil: Date.now() + 10 * 60_000, rules: { blockSites: ["youtube.com"] } };
  const h = await startOn(YT, { storage: { local: { state } } });
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(5);
  assert.match(h.lastPlayer().notice, BLOCKED);
});

test("while disconnected, nothing counts as safe to send and the cloud voice is not offered", async (t) => {
  const h = await startOn("https://news.example/a", { app: fakeApp(FOCUS) });
  t.after(() => h.dispose());
  await h.dropPort();
  assert.equal((await h.send({ type: "page.url", url: "https://news.example/a" })).private, false);
  await h.cmd("start");
  await tick(5);
  assert.equal(h.toApp.filter((m) => m.type === "tts.speak").length, 0, "reads with the Mac voice");
  assert.equal(h.spoken[0].text, "One.");
});

test("an unblocked site still reads while the app is closed", async (t) => {
  const h = await startOn("https://news.example/a", { storage: { local: { state: FOCUS } } });
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(5);
  assert.equal(h.spoken[0].text, "One.");
});

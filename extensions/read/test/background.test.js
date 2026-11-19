import test from "node:test";
import assert from "node:assert/strict";
import { articleTab, fakeApp, html, startRead, tick } from "./harness.js";

const A = "https://news.example/a";
const B = "https://news.example/b";

async function playing(opts = {}) {
  const tab = articleTab(1, A, ["One.", "Two.", "Three."]);
  const h = await startRead({ app: fakeApp(), tabs: [tab], windows: [{ id: 1, focused: true }], ...opts });
  await h.cmd("start");
  await tick(5);
  return h;
}

test("a rate change that lands after a pause does not start speaking again", async (t) => {
  const h = await playing();
  t.after(() => h.dispose());
  const rate = h.send({ type: "settings.set", rate: 1.5 });
  await h.cmd("toggle");
  await rate;
  await tick(5);
  assert.equal(h.lastPlayer().playing, false);
  assert.equal(h.speaking, null, "no utterance outlives the pause");
});

test("speech cut off from outside pauses the player, stops the clock and tells the app", async (t) => {
  const realNow = Date.now;
  let now = 1_000_000;
  Date.now = () => now;
  t.after(() => { Date.now = realNow; });
  const h = await playing();
  t.after(() => h.dispose());
  now += 10_000;
  await h.interruptFromOutside();
  assert.equal(h.lastPlayer().playing, false);
  assert.equal(h.statuses().at(-1), "paused");
  now += 90_000;
  await h.cmd("toggle"); // play again
  now += 5_000;
  await h.cmd("toggle"); // pause
  assert.deepEqual(h.logs().map((l) => l.seconds), [10, 5], "idle time is not logged as listening");
});

test("finishing an article stops speech", async (t) => {
  const h = await playing();
  t.after(() => h.dispose());
  await h.cmd("seek", 2);
  await h.cmd("next");
  await tick(5);
  assert.equal(h.speaking, null);
  assert.equal(h.lastPlayer().playing, false);
  assert.equal(h.statuses().at(-1), "stopped");
});

test("two finishes at once advance the queue by one article", async (t) => {
  let fetches = 0;
  const fetch = async (url) => {
    fetches++;
    await tick(20);
    return html(`Queued ${url}. Second.`);
  };
  const h = await playing({ fetch, storage: { local: { queue: [{ url: "https://q.example/1", title: "Q1" }, { url: "https://q.example/2", title: "Q2" }] } } });
  t.after(() => h.dispose());
  await h.cmd("seek", 2);
  await Promise.all([h.cmd("next"), h.cmd("next")]);
  await tick(80);
  assert.equal(fetches, 1);
  assert.deepEqual(h.local.queue.map((q) => q.title), ["Q2"]);
});

test("a queued article that fails to load stays in the queue", async (t) => {
  const fetch = async () => { throw new Error("offline"); };
  const h = await playing({ fetch, storage: { local: { queue: [{ url: "https://q.example/1", title: "Q1" }] } } });
  t.after(() => h.dispose());
  await h.cmd("seek", 2);
  await h.cmd("next");
  await tick(20);
  assert.deepEqual(h.local.queue.map((q) => q.title), ["Q1"]);
  assert.match(h.lastPlayer().notice, /Q1/);
});

test("Read to me does not start in a tab whose window is not focused", async (t) => {
  const tab = articleTab(5, B, ["Hello there.", "Again."], { windowId: 2 });
  const h = await startRead({ app: fakeApp(), tabs: [tab], windows: [{ id: 2, focused: false }], storage: { local: { settings: { readToMe: true } } } });
  t.after(() => h.dispose());
  h.tabsOnUpdated.fire(5, { status: "complete" }, tab);
  await tick(20);
  assert.equal(h.spoken.length, 0);
});

test("a worker restart with a saved session tells the app reading is paused", async (t) => {
  const session = { tabId: 1, url: A, title: "A", source: "page", paragraphs: ["One. Two."], items: [{ text: "One.", p: 0 }, { text: "Two.", p: 0 }], index: 1 };
  const h = await startRead({ app: fakeApp(), storage: { session: { session } } });
  t.after(() => h.dispose());
  await tick(10);
  assert.equal(h.statuses().at(-1), "paused");
});

test("loading another article without playing reports that reading paused", async (t) => {
  const app = fakeApp({ mode: "off" }, (msg, reply) => {
    if (msg.type === "scene.candidates") reply({ type: "scene.candidates.result", id: msg.id, candidates: [] });
  });
  const tab = articleTab(1, A, ["One.", "Two.", "Three."]);
  const h = await startRead({ app, tabs: [tab], windows: [{ id: 1, focused: true }] });
  t.after(() => h.dispose());
  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-selection", selectionText: "Picked words. More words." }, tab);
  await tick(10);
  assert.equal(h.statuses().at(-1), "playing");
  await h.send({ type: "scene.candidates" }); // loads the page's article without reading it
  assert.equal(h.lastPlayer().playing, false);
  assert.equal(h.statuses().at(-1), "paused");
  assert.equal(h.toApp.filter((m) => m.type === "read.status").at(-1).title, "Article 1");
});

test("the saved position survives the 500-entry trim when its key is the oldest", async (t) => {
  const positions = { [A]: 1 };
  for (let i = 0; i < 500; i++) positions[`https://old.example/${i}`] = 3;
  const h = await playing({ storage: { local: { positions } } });
  t.after(() => h.dispose());
  await tick(10);
  assert.equal(h.speaking.text, "Two.", "resumed at the saved sentence");
  assert.equal(h.local.positions[A], 1);
  assert.equal(Object.keys(h.local.positions).length, 500);
});

test("the panel snapshot keeps request failures apart from the status line", async (t) => {
  const h = await playing();
  t.after(() => h.dispose());
  h.speaking.options.onEvent({ type: "error", errorMessage: "synth died" });
  await tick(5);
  const p = await h.send({ type: "player.get" });
  assert.equal(p.playing, false);
  assert.match(p.notice, /synth died/);
  assert.equal("error" in p, false, "an `error` key reads as a failed request in the side panel");
});

test("two links read at once share the one offscreen document in turn", async (t) => {
  const fetch = async (url) => { await tick(2); return html(`From ${url}. Done.`); };
  const h = await startRead({ app: fakeApp(), fetch });
  t.after(() => h.dispose());
  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-link", linkUrl: "https://l.example/1" }, { id: 9, windowId: 1, url: "https://x.example/" });
  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-link", linkUrl: "https://l.example/2" }, { id: 9, windowId: 1, url: "https://x.example/" });
  await tick(80);
  assert.deepEqual(h.offscreen.errors, []);
  assert.equal(h.offscreen.created, 2);
  assert.equal(h.lastPlayer().notice, "");
  assert.equal(h.lastPlayer().url, "https://l.example/2");
});

test("a page body over the size cap is refused before parsing", async (t) => {
  const big = "x".repeat(6 * 1024 * 1024);
  const fetch = async () => new Response(new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode(`<p>${big}</p>`)); c.close(); } }), { headers: { "content-type": "text/html" } });
  const h = await startRead({ app: fakeApp(), fetch });
  t.after(() => h.dispose());
  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-link", linkUrl: "https://l.example/huge" }, { id: 9, windowId: 1, url: "https://x.example/" });
  await tick(50);
  assert.equal(h.offscreen.created, 0);
  assert.match(h.lastPlayer().notice, /too large/);
});

test("stopping while a link is loading cancels it before playback starts", async (t) => {
  let resolveFetch;
  const fetch = () => new Promise((resolve) => { resolveFetch = resolve; });
  const h = await startRead({ app: fakeApp(), fetch });
  t.after(() => h.dispose());

  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-link", linkUrl: "https://l.example/slow" }, { id: 9, windowId: 1, url: "https://x.example/" });
  await tick(10);
  await h.cmd("stop");
  resolveFetch(html("Arrived after stop. Should stay quiet."));
  await tick(30);

  assert.equal(h.speaking, null, "cancelled load must not start speaking");
  assert.equal(h.lastPlayer().playing, false);
  assert.equal(h.lastPlayer().url, "");
});

test("a slower earlier link load cannot overwrite a newer link load", async (t) => {
  let resolveSlow;
  const fetch = (url) => {
    if (url.endsWith("/slow")) return new Promise((resolve) => { resolveSlow = resolve; });
    return Promise.resolve(html("Fast article. Second sentence."));
  };
  const h = await startRead({ app: fakeApp(), fetch });
  t.after(() => h.dispose());

  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-link", linkUrl: "https://l.example/slow" }, { id: 9, windowId: 1, url: "https://x.example/" });
  await tick(10);
  h.chrome.contextMenus.onClicked.fire({ menuItemId: "read-link", linkUrl: "https://l.example/fast" }, { id: 9, windowId: 1, url: "https://x.example/" });
  await tick(30);
  assert.equal(h.lastPlayer().url, "https://l.example/fast");

  resolveSlow(html("Slow article. Late arrival."));
  await tick(30);
  assert.equal(h.lastPlayer().url, "https://l.example/fast", "late earlier load must not clobber newer load");
  assert.equal(h.speaking.text, "Fast article.");
});

test("stopping while a queued item is loading keeps it in the queue and stays stopped", async (t) => {
  let resolveFetch;
  const fetch = () => new Promise((resolve) => { resolveFetch = resolve; });
  const h = await startRead({ app: fakeApp(), fetch, storage: { local: { queue: [{ url: "https://q.example/1", title: "Q1" }] } } });
  t.after(() => h.dispose());

  const playReq = h.send({ type: "queue.play", index: 0 });
  await tick(10);
  await h.cmd("stop");
  resolveFetch(html("Queued content. Too late."));
  await playReq;
  await tick(20);

  assert.equal(h.speaking, null);
  assert.deepEqual(h.local.queue.map((q) => q.title), ["Q1"], "cancelled queue load must not drop the item");
});

test("stopping while playing flushes listening time without double-logging", async (t) => {
  const realNow = Date.now;
  let now = 2_000_000;
  Date.now = () => now;
  t.after(() => { Date.now = realNow; });

  const h = await playing();
  t.after(() => h.dispose());
  now += 25_000;
  await h.cmd("stop");
  assert.deepEqual(h.logs().map((l) => l.seconds), [25], "stop while playing flushes accumulated listening time");
  assert.equal(h.statuses().at(-1), "stopped");

  // Stopping again while already idle must not emit a duplicate log.
  now += 10_000;
  await h.cmd("stop");
  assert.deepEqual(h.logs().map((l) => l.seconds), [25]);
});

// ---- cloud voice -----------------------------------------------------------------------------

const CLOUD = "kairos:cloud";
const ELEVEN = { provider: "elevenlabs", name: "ElevenLabs" };

async function cloudPlaying({ rate = 1, failOn = null } = {}) {
  const pending = [];
  const state = { mode: "off", cloudVoice: ELEVEN };
  const app = fakeApp(state, (msg, reply) => {
    if (msg.type === "tts.speak") {
      if (msg.text === failOn) reply({ type: "error", id: msg.id, message: "quota exceeded" });
      else pending.push({ msg, done: (stopped = false) => reply({ type: "tts.done", id: msg.id, stopped }) });
    }
  });
  const tab = articleTab(1, A, ["One.", "Two.", "Three."]);
  const h = await startRead({ app, tabs: [tab], windows: [{ id: 1, focused: true }], storage: { local: { settings: { voice: CLOUD, rate } } } });
  await h.cmd("start");
  await tick(10);
  return { h, pending, state };
}
const ttsSent = (h, type) => h.toApp.filter((m) => m.type === type);

test("with the cloud voice chosen, each sentence goes to Kairos Desktop with the next one prefetched", async (t) => {
  const { h, pending } = await cloudPlaying();
  t.after(() => h.dispose());
  assert.equal(h.spoken.length, 0, "chrome.tts stays quiet");
  assert.deepEqual(ttsSent(h, "tts.speak").map((m) => [m.text, m.rate]), [["One.", 1]]);
  assert.deepEqual(ttsSent(h, "tts.prefetch").map((m) => m.text), ["Two."]);
  assert.equal(h.sentToTabs.filter((m) => m.msg.type === "kr.highlight").at(-1).msg.text, "One.");
  assert.equal(h.statuses().at(-1), "playing");
  pending.shift().done();
  await tick(10);
  assert.deepEqual(ttsSent(h, "tts.speak").map((m) => m.text), ["One.", "Two."]);
  assert.equal(h.sentToTabs.filter((m) => m.msg.type === "kr.highlight").at(-1).msg.text, "Two.");
  const p = await h.send({ type: "player.get" });
  assert.deepEqual(p.cloudVoice, ELEVEN);
  assert.equal(p.index, 1);
});

test("the cloud voice speed is clamped to 2x", async (t) => {
  const { h } = await cloudPlaying({ rate: 2.5 });
  t.after(() => h.dispose());
  assert.equal(ttsSent(h, "tts.speak")[0].rate, 2);
  assert.equal((await h.send({ type: "player.get" })).cloudRate, 2);
});

test("a cloud sentence that fails falls back to the default Mac voice from that sentence, said once", async (t) => {
  const { h, pending } = await cloudPlaying({ failOn: "Two." });
  t.after(() => h.dispose());
  pending.shift().done();
  await tick(10);
  assert.equal(h.speaking.text, "Two.");
  assert.equal(h.speaking.options.voiceName, undefined, "the system default voice");
  const notice = h.lastPlayer().notice;
  assert.match(notice, /Mac voice/);
  await h.endUtterance();
  assert.equal(h.speaking.text, "Three.");
  assert.equal(ttsSent(h, "tts.speak").length, 2, "no more cloud attempts in this article");
  assert.equal(h.lastPlayer().notice, notice);
  assert.equal(h.lastPlayer().playing, true);
});

test("losing Kairos Desktop mid-sentence falls back to the Mac voice on the same sentence", async (t) => {
  const { h } = await cloudPlaying();
  t.after(() => h.dispose());
  await h.dropPort();
  assert.equal(h.speaking.text, "One.");
  assert.equal(h.lastPlayer().playing, true);
  assert.match(h.lastPlayer().notice, /Mac voice/);
});

test("the cloud voice going away in a state push falls back", async (t) => {
  const { h } = await cloudPlaying();
  t.after(() => h.dispose());
  await h.push({ type: "state", state: { mode: "off", cloudVoice: null } });
  assert.equal(h.speaking.text, "One.");
  assert.ok(ttsSent(h, "tts.stop").length >= 1, "the cloud sentence is stopped first");
});

test("pausing a cloud sentence stops it in the app and its late done is ignored", async (t) => {
  const { h, pending } = await cloudPlaying();
  t.after(() => h.dispose());
  await h.cmd("toggle");
  assert.equal(ttsSent(h, "tts.stop").length, 1);
  pending.shift().done(true);
  await tick(10);
  assert.equal(h.lastPlayer().playing, false);
  assert.equal(ttsSent(h, "tts.speak").length, 1);
  assert.equal(h.statuses().at(-1), "paused");
  await h.cmd("toggle");
  await tick(5);
  assert.deepEqual(ttsSent(h, "tts.speak").map((m) => m.text), ["One.", "One."], "resumes from the same sentence");
});

test("the panel snapshot says why Kairos Desktop is out of reach", async (t) => {
  const app = (msg, reply) => { if (msg.type === "hello") reply({ type: "error", message: "Kairos Desktop is not running" }); };
  const h = await startRead({ app });
  t.after(() => h.dispose());
  await h.dropPort();
  const p = await h.send({ type: "player.get" });
  assert.equal(p.connected, false);
  assert.match(p.connection.text, /not running/);
});

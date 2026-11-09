import test from "node:test";
import assert from "node:assert/strict";
import { articleTab, fakeApp, startRead, tick } from "./harness.js";

const CLOUD = "kairos:cloud";
const ELEVEN = { provider: "elevenlabs", name: "ElevenLabs" };
const LISTED = "https://mail.example/inbox/1";
const OPEN = "https://news.example/a";
const offMac = (h) => h.toApp.filter((m) => /^(tts\.(speak|prefetch)|scene\.)/.test(m.type));

function appWith(neverAnalyse) {
  const pending = [];
  const state = { mode: "off", cloudVoice: ELEVEN, rules: { neverAnalyse } };
  const app = fakeApp(state, (msg, reply) => {
    if (msg.type === "tts.speak") pending.push({ msg, done: (stopped = false) => reply({ type: "tts.done", id: msg.id, stopped }) });
    if (msg.type === "scene.candidates") reply({ type: "scene.candidates.result", id: msg.id, candidates: [{ name: "Engine", kind: "machine", why: "w" }] });
    if (msg.type === "scene.generate") reply({ type: "scene.result", id: msg.id, html: "<!doctype html><html></html>" });
    if (msg.type === "scene.edit") reply({ type: "scene.result", id: msg.id, html: "<!doctype html><html></html>" });
  });
  return { app, pending, state };
}

async function reading(url, neverAnalyse, extra = {}) {
  const fake = appWith(neverAnalyse);
  const tab = articleTab(1, url, ["One.", "Two.", "Three."]);
  const h = await startRead({ app: fake.app, tabs: [tab], windows: [{ id: 1, focused: true }], storage: { local: { settings: { voice: CLOUD, ...(extra.settings || {}) } } } });
  return { h, tab, ...fake };
}

test("a page on the never-analyse list is read with the Mac voice and nothing is sent to the cloud voice", async (t) => {
  const { h } = await reading(LISTED, ["mail.example"]);
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(10);
  assert.equal(h.speaking.text, "One.", "chrome.tts speaks");
  assert.equal(h.speaking.options.voiceName, undefined, "the system default voice");
  assert.deepEqual(offMac(h), []);
  const p = await h.send({ type: "player.get" });
  assert.equal(p.private, true);
});

test("the list covers subdomains, as site rules do", async (t) => {
  const { h } = await reading("https://app.mail.example/x", ["mail.example"]);
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(10);
  assert.deepEqual(offMac(h), []);
});

test("a page that is not on the list still uses the cloud voice, with its url", async (t) => {
  const { h, pending } = await reading(OPEN, ["mail.example"]);
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(10);
  assert.equal(h.spoken.length, 0);
  const speak = h.toApp.filter((m) => m.type === "tts.speak");
  assert.deepEqual(speak.map((m) => [m.text, m.url]), [["One.", OPEN]]);
  assert.deepEqual(h.toApp.filter((m) => m.type === "tts.prefetch").map((m) => [m.text, m.url]), [["Two.", OPEN]]);
  assert.equal((await h.send({ type: "player.get" })).private, false);
  assert.equal(pending.length, 1);
});

test("adding the page's site to the list mid-sentence stops the cloud voice and carries on with the Mac voice", async (t) => {
  const { h, state } = await reading(OPEN, []);
  t.after(() => h.dispose());
  await h.cmd("start");
  await tick(10);
  assert.equal(h.toApp.filter((m) => m.type === "tts.speak").length, 1);
  await h.push({ type: "state", state: { ...state, rules: { neverAnalyse: ["news.example"] } } });
  assert.ok(h.toApp.some((m) => m.type === "tts.stop"), "the cloud sentence is stopped");
  assert.equal(h.speaking.text, "One.");
  assert.equal(h.toApp.filter((m) => m.type === "tts.speak").length, 1, "no new cloud request");
  assert.equal(h.lastPlayer().playing, true);
});

test("Read to me never starts the cloud voice on a listed page", async (t) => {
  const { h, tab } = await reading(LISTED, ["mail.example"], { settings: { readToMe: true } });
  t.after(() => h.dispose());
  h.tabsOnUpdated.fire(1, { status: "complete" }, tab);
  await tick(30);
  assert.equal(h.speaking.text, "One.");
  assert.deepEqual(offMac(h), []);
});

test("3D scenes are refused for a listed page without sending anything", async (t) => {
  const { h } = await reading(LISTED, ["mail.example"]);
  t.after(() => h.dispose());
  assert.match((await h.send({ type: "scene.candidates" })).error, /never-analyse/i);
  for (const msg of [{ type: "scene.build", subject: "Engine" }, { type: "scene.change", url: LISTED, id: "s1", instruction: "bigger" }]) {
    await h.send(msg);
    await tick(20);
    const job = (await h.send({ type: "player.get" })).sceneJob;
    assert.equal(job.status, "error", msg.type);
    assert.match(job.error, /never-analyse/i, msg.type);
    await h.send({ type: "scene.ack", id: job.id });
  }
  assert.deepEqual(offMac(h), []);
});

test("scene requests carry the page url", async (t) => {
  const { h } = await reading(OPEN, ["mail.example"]);
  t.after(() => h.dispose());
  await h.send({ type: "scene.candidates" });
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(20);
  assert.deepEqual(h.toApp.filter((m) => m.type === "scene.candidates" || m.type === "scene.generate").map((m) => m.url), [OPEN, OPEN]);
});

test("page.url says whether the page the panel is showing is on the list", async (t) => {
  const { h } = await reading(LISTED, ["mail.example"]);
  t.after(() => h.dispose());
  assert.deepEqual(await h.send({ type: "page.url" }), { url: LISTED, private: true });
});

test("before the app has sent its settings no page is known to be safe to send", async (t) => {
  const app = (msg, reply) => { if (msg.type === "hello") reply({ type: "read.command", command: "noop" }); };
  const tab = articleTab(1, OPEN, ["One.", "Two."]);
  const h = await startRead({ app, tabs: [tab], windows: [{ id: 1, focused: true }] });
  t.after(() => h.dispose());
  const reply = await h.send({ type: "scene.candidates" });
  assert.match(reply.error, /settings|never-analyse/i);
  assert.deepEqual(offMac(h), []);
});

import test from "node:test";
import assert from "node:assert/strict";
import { articleTab, fakeApp, startRead, tick } from "./harness.js";
import { scenesFor } from "../lib/scenes.js";

const URL_A = "https://news.example/a";
const DOC = (n = 1) => `<!doctype html><html><body>scene ${n}</body></html>`;

/** A fake app whose scene.generate / scene.edit replies the test controls. */
function sceneApp({ reply = true } = {}) {
  const held = [];
  const cancels = [];
  let n = 0;
  const app = fakeApp({ mode: "off", rules: {} }, (msg, send) => {
    if (msg.type === "scene.cancel") {
      cancels.push(msg.id);
      // Kairos Desktop stops the build and answers the original request with an error.
      const build = held.find((b) => b.msg.id === msg.id);
      if (build) build.fail("The scene build was cancelled.");
      return;
    }
    if (msg.type !== "scene.generate" && msg.type !== "scene.edit") return;
    const answer = (extra = {}) => send({ type: "scene.result", id: msg.id, html: DOC(++n), ...extra });
    if (reply) answer();
    else held.push({ msg, answer, fail: (message) => send({ type: "error", id: msg.id, message }) });
  });
  return { app, held, cancels };
}

async function start(opts = {}, tabUrl = URL_A) {
  const fake = sceneApp(opts);
  const tab = articleTab(1, tabUrl, ["One.", "Two."]);
  const h = await startRead({ app: fake.app, tabs: [tab], windows: [{ id: 1, focused: true }] });
  return { h, ...fake };
}
const job = (h) => h.send({ type: "player.get" }).then((p) => p.sceneJob);
const saved = (h, url = URL_A) => scenesFor(h.chrome.storage.local, url);

test("a scene finishes and is saved with no side panel listening", async (t) => {
  const { h } = await start();
  t.after(() => h.dispose());
  const started = await h.send({ type: "scene.build", subject: "Engine" });
  assert.equal(started.error, undefined);
  await tick(30);
  const list = await saved(h);
  assert.deepEqual(list.map((s) => s.subject), ["Engine"]);
  const j = await job(h);
  assert.equal(j.status, "done");
  assert.equal(j.sceneId, list[0].id);
  assert.equal(j.url, URL_A);
});

test("a running build is reported with its start time, and a second build is refused", async (t) => {
  const { h, held } = await start({ reply: false });
  t.after(() => h.dispose());
  const before = Date.now();
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(20);
  const j = await job(h);
  assert.equal(j.status, "running");
  assert.ok(j.startedAt >= before && j.startedAt <= Date.now());
  assert.equal(j.subject, "Engine");
  const second = await h.send({ type: "scene.build", subject: "Bridge" });
  assert.match(second.error, /already being built/);
  assert.equal(held.length, 1);
});

test("a build that finishes later is saved, and the panel finds it by asking again", async (t) => {
  const { h, held } = await start({ reply: false });
  t.after(() => h.dispose());
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(20);
  assert.equal((await saved(h)).length, 0);
  held[0].answer();
  await tick(20);
  assert.equal((await job(h)).status, "done");
  assert.equal((await saved(h)).length, 1);
});

test("Cancel stops waiting: a late answer is not saved and the next build can start", async (t) => {
  const { h, held } = await start({ reply: false });
  t.after(() => h.dispose());
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(20);
  await h.send({ type: "scene.cancel" });
  assert.equal(await job(h), null);
  held[0].answer();
  await tick(20);
  assert.equal((await saved(h)).length, 0);
  assert.equal((await h.send({ type: "scene.build", subject: "Engine" })).error, undefined);
});

test("Cancel asks Kairos Desktop to stop that build, by its request id", async (t) => {
  const { h, held, cancels } = await start({ reply: false });
  t.after(() => h.dispose());
  await h.send({ type: "scene.build", subject: "Pendulum clock" });
  await tick(20);
  const asked = held[0].msg;
  assert.equal(asked.type, "scene.generate");
  assert.match(asked.id, /^read-scene-/);
  await h.send({ type: "scene.cancel" });
  await tick(20);
  assert.deepEqual(cancels.slice(0, 1), [asked.id]);
  assert.equal(await job(h), null, "the app's 'cancelled' answer does not bring the job back");
  assert.equal((await saved(h)).length, 0);
});

test("a failed build leaves the reason on the job until it is acknowledged", async (t) => {
  const { h, held } = await start({ reply: false });
  t.after(() => h.dispose());
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(20);
  held[0].fail("No API key");
  await tick(20);
  const j = await job(h);
  assert.deepEqual([j.status, j.error], ["error", "No API key"]);
  await h.send({ type: "scene.ack", id: j.id });
  assert.equal(await job(h), null);
});

test("a scene lost to a dropped connection says why instead of 'not connected'", async (t) => {
  const { h } = await start({ reply: false });
  t.after(() => h.dispose());
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(20);
  await h.dropPort();
  assert.match((await job(h)).error, /1 MB/);
});

test("changing a scene replaces it under the same id", async (t) => {
  const { h } = await start();
  t.after(() => h.dispose());
  await h.send({ type: "scene.build", subject: "Engine" });
  await tick(30);
  const [first] = await saved(h);
  await h.send({ type: "scene.ack", id: (await job(h)).id });
  const sent = h.toApp.length;
  const r = await h.send({ type: "scene.change", url: URL_A, id: first.id, instruction: "bigger" });
  assert.equal(r.error, undefined);
  await tick(30);
  const list = await saved(h);
  assert.deepEqual(list.map((s) => s.id), [first.id]);
  assert.notEqual(list[0].html, first.html);
  const edit = h.toApp.slice(sent).find((m) => m.type === "scene.edit");
  assert.equal(edit.html, first.html, "the stored scene is what is edited");
  assert.equal(edit.instruction, "bigger");
});

test("changing a scene that is gone says so", async (t) => {
  const { h } = await start();
  t.after(() => h.dispose());
  await h.send({ type: "scene.change", url: URL_A, id: "nope", instruction: "bigger" });
  await tick(20);
  assert.match((await job(h)).error, /no longer saved/);
});

test("only the 20 newest scenes are kept when builds keep finishing", async (t) => {
  const { h } = await start();
  t.after(() => h.dispose());
  for (let i = 0; i < 22; i++) {
    await h.send({ type: "scene.build", subject: `S${i}` });
    await tick(15);
    await h.send({ type: "scene.ack", id: (await job(h)).id });
  }
  const list = await saved(h);
  assert.equal(list.length, 20);
  assert.equal(list[0].subject, "S21");
});

test("a build interrupted by a worker restart is reported as such", async (t) => {
  const sceneJob = { id: "j1", kind: "generate", subject: "Engine", url: URL_A, startedAt: Date.now() - 5000, status: "running", error: "", sceneId: "" };
  const h = await startRead({ app: fakeApp(), storage: { session: { sceneJob } } });
  t.after(() => h.dispose());
  await tick(10);
  const j = await job(h);
  assert.equal(j.status, "error");
  assert.match(j.error, /interrupted/);
});

test("a finished scene waits for the panel across a worker restart", async (t) => {
  const sceneJob = { id: "j1", kind: "generate", subject: "Engine", url: URL_A, startedAt: Date.now() - 5000, status: "done", error: "", sceneId: "s1" };
  const h = await startRead({ app: fakeApp(), storage: { session: { sceneJob } } });
  t.after(() => h.dispose());
  await tick(10);
  assert.equal((await job(h)).status, "done");
});

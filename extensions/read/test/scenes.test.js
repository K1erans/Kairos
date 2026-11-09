import test from "node:test";
import assert from "node:assert/strict";
import { deleteScene, saveScene, scenesFor } from "../lib/scenes.js";

function area() {
  const data = {};
  const reads = [];
  return {
    data,
    reads,
    get: async (keys) => {
      const out = {};
      for (const k of [].concat(keys)) {
        reads.push(k);
        if (k in data) out[k] = structuredClone(data[k]);
      }
      return out;
    },
    set: async (obj) => Object.assign(data, structuredClone(obj)),
    remove: async (keys) => { for (const k of [].concat(keys)) delete data[k]; },
  };
}
const stored = (a) => Object.entries(a.data).filter(([k]) => k.startsWith("scenes:")).flatMap(([, list]) => list);

test("only the 20 most recent scenes are kept, across articles", async () => {
  const a = area();
  for (let i = 0; i < 25; i++) await saveScene(a, `https://site.example/${i % 3}`, { id: `s${i}`, subject: "x", html: "<html></html>" }, { now: i });
  const kept = stored(a).map((s) => s.id).sort((x, y) => Number(x.slice(1)) - Number(y.slice(1)));
  assert.deepEqual(kept, Array.from({ length: 20 }, (_, i) => `s${i + 5}`));
});

test("an article's scenes are read without loading the others", async () => {
  const a = area();
  await saveScene(a, "https://a.example/post#top", { id: "1", subject: "Engine", html: "<html>a</html>" });
  await saveScene(a, "https://b.example/post", { id: "2", subject: "Bridge", html: "<html>b</html>" });
  a.reads.length = 0;
  const list = await scenesFor(a, "https://a.example/post");
  assert.deepEqual(list.map((s) => s.subject), ["Engine"]);
  assert.deepEqual(a.reads, ["scenes:https://a.example/post"]);
});

test("editing a scene replaces it, and deleting the last one removes the article key", async () => {
  const a = area();
  await saveScene(a, "https://a.example/", { id: "1", subject: "Engine", html: "v1" });
  await saveScene(a, "https://a.example/", { id: "1", subject: "Engine", html: "v2" });
  assert.deepEqual((await scenesFor(a, "https://a.example/")).map((s) => s.html), ["v2"]);
  await deleteScene(a, "https://a.example/", "1");
  assert.deepEqual(Object.keys(a.data), ["sceneIndex"]);
  assert.deepEqual(a.data.sceneIndex, []);
});

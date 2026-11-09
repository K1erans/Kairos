import test from "node:test";
import assert from "node:assert/strict";
import { createBridge } from "../lib/bridge.js";

function fakePort() {
  const listeners = { message: [], disconnect: [] };
  return {
    sent: [],
    onMessage: { addListener: (fn) => listeners.message.push(fn) },
    onDisconnect: { addListener: (fn) => listeners.disconnect.push(fn) },
    postMessage(msg) { this.sent.push(msg); },
    disconnect() {},
    receive: (msg) => listeners.message.forEach((fn) => fn(msg)),
    drop: () => listeners.disconnect.forEach((fn) => fn()),
  };
}

test("says hello, reports connected on the first reply and routes pushes", () => {
  const port = fakePort();
  const pushes = [];
  const status = [];
  const bridge = createBridge({ client: "shield", version: "1", connect: () => port, onMessage: (m) => pushes.push(m), onStatus: (s) => status.push(s) });
  bridge.start();
  assert.deepEqual(port.sent[0], { type: "hello", client: "shield", version: "1" });
  assert.equal(bridge.isConnected(), false);
  port.receive({ type: "state", state: { mode: "focus" } });
  assert.equal(bridge.isConnected(), true);
  assert.deepEqual(status, [true]);
  assert.equal(pushes.length, 1);
  bridge.stop();
});

test("request resolves with the reply that echoes its id, and rejects on error replies", async () => {
  const port = fakePort();
  const pushes = [];
  const bridge = createBridge({ client: "read", version: "1", connect: () => port, onMessage: (m) => pushes.push(m) });
  bridge.start();
  const p = bridge.request({ type: "jev.check", url: "https://a.com" });
  const sent = port.sent[1];
  assert.equal(sent.type, "jev.check");
  assert.ok(typeof sent.id === "string" && sent.id.length > 0);
  port.receive({ type: "jev.result", id: sent.id, block: true });
  assert.equal((await p).block, true);
  assert.equal(pushes.length, 0, "replies are not delivered as pushes");

  const q = bridge.request({ type: "scene.generate" });
  port.receive({ type: "error", id: port.sent[2].id, message: "No API key" });
  await assert.rejects(q, /No API key/);
  bridge.stop();
});

test("request times out", async () => {
  const port = fakePort();
  const bridge = createBridge({ client: "shield", version: "1", connect: () => port });
  bridge.start();
  await assert.rejects(bridge.request({ type: "jev.check" }, 20), /Timed out/);
  bridge.stop();
});

test("disconnect fails pending requests and reconnects with backoff", async () => {
  const ports = [];
  const delays = [];
  let fire = null;
  const bridge = createBridge({
    client: "shield",
    version: "1",
    connect: () => { const p = fakePort(); ports.push(p); return p; },
    setTimeout: (fn, ms) => {
      if (ms >= 1000) { delays.push(ms); fire = fn; return 0; }
      return setTimeout(fn, ms);
    },
  });
  bridge.start();
  ports[0].receive({ type: "state", state: {} });
  const p = bridge.request({ type: "jev.check" }, 500);
  ports[0].drop();
  await assert.rejects(p, /not connected/);
  assert.equal(bridge.isConnected(), false);
  await assert.rejects(bridge.request({ type: "jev.check" }), /not connected/);
  assert.equal(bridge.send({ type: "blockEvent" }), false);
  fire();
  assert.equal(ports.length, 2);
  ports[1].drop();
  fire();
  ports[2].drop();
  assert.deepEqual(delays.slice(0, 3), [1000, 2000, 4000]);
  bridge.stop();
});

test("messages sent with queue while the app is away are delivered on reconnect, newest 50 kept", () => {
  const ports = [];
  let fire = null;
  const saved = [];
  const bridge = createBridge({
    client: "shield",
    version: "1",
    connect: () => { const p = fakePort(); ports.push(p); return p; },
    setTimeout: (fn, ms) => { if (ms >= 1000) { fire = fn; return 0; } return setTimeout(fn, ms); },
    onOutbox: (list) => saved.push(list.length),
  });
  bridge.start();
  ports[0].drop();
  assert.equal(bridge.send({ type: "rule.addElement", selector: "#a" }, { queue: true }), false);
  assert.equal(bridge.send({ type: "blockEvent" }), false, "unqueued messages are dropped");
  for (let i = 0; i < 60; i++) bridge.send({ type: "rule.addElement", selector: `#n${i}` }, { queue: true });
  fire();
  assert.deepEqual(ports[1].sent.map((m) => m.type), ["hello"], "nothing is flushed before the app answers");
  ports[1].receive({ type: "state", state: {} });
  const flushed = ports[1].sent.slice(1);
  assert.equal(flushed.length, 50);
  assert.equal(flushed[0].selector, "#n10");
  assert.equal(flushed.at(-1).selector, "#n59");
  assert.equal(saved.at(-1), 0, "the persisted outbox is emptied after the flush");
  bridge.stop();
});

test("the host's 'Kairos Desktop is not running' notice does not count as connected", () => {
  const port = fakePort();
  const status = [];
  const bridge = createBridge({ client: "read", version: "1", connect: () => port, onStatus: (s) => status.push(s) });
  bridge.start();
  port.receive({ type: "error", message: "Kairos Desktop is not running" });
  assert.equal(bridge.isConnected(), false);
  assert.deepEqual(status, []);
  bridge.stop();
});

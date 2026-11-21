import test from "node:test";
import assert from "node:assert/strict";
import { RECONNECT_ALARM, createBridge, describeConnection, saysNotRunning } from "../lib/bridge.js";

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

// ---- why the app is not reachable -------------------------------------------------------------

function problemBridge({ detail = "" } = {}) {
  const ports = [];
  const problems = [];
  let fire = null;
  globalThis.chrome = { runtime: { get lastError() { return detail ? { message: detail } : undefined; } } };
  const bridge = createBridge({
    client: "shield",
    version: "1",
    connect: () => { const p = fakePort(); ports.push(p); return p; },
    setTimeout: (fn, ms) => { if (ms >= 1000) { fire = fn; return 0; } return setTimeout(fn, ms); },
    onProblem: (p) => problems.push(p),
  });
  bridge.start();
  return { bridge, ports, problems, retry: () => fire() };
}

test("the host saying Kairos Desktop is not running is reported as that, not as a lost connection", () => {
  const { bridge, ports, problems } = problemBridge({ detail: "Native host has exited." });
  ports[0].receive({ type: "error", message: "Kairos Desktop is not running" });
  ports[0].drop();
  assert.equal(bridge.problem(), "app-not-running");
  assert.deepEqual(problems, ["app-not-running"]);
  bridge.stop();
});

test("a missing native host is reported as the bridge not being installed", () => {
  const { bridge, ports } = problemBridge({ detail: "Specified native messaging host not found." });
  ports[0].drop();
  assert.equal(bridge.problem(), "bridge-missing");
  bridge.stop();
  const forbidden = problemBridge({ detail: "Access to the specified native messaging host is forbidden." });
  forbidden.ports[0].drop();
  assert.equal(forbidden.bridge.problem(), "bridge-missing");
  forbidden.bridge.stop();
});

test("losing a connection that was working is 'retrying'; reconnecting clears the problem", () => {
  const { bridge, ports, problems, retry } = problemBridge({ detail: "Native host has exited." });
  ports[0].receive({ type: "state", state: {} });
  assert.equal(bridge.problem(), null);
  ports[0].drop();
  assert.equal(bridge.problem(), "retrying");
  retry();
  ports[1].receive({ type: "state", state: {} });
  assert.equal(bridge.problem(), null);
  assert.deepEqual(problems, ["retrying", null]);
  bridge.stop();
});

test("an earlier 'not running' notice does not stick to a later, different failure", () => {
  const { bridge, ports, retry } = problemBridge({ detail: "Native host has exited." });
  ports[0].receive({ type: "error", message: "Kairos Desktop is not running" });
  ports[0].drop();
  retry();
  ports[1].receive({ type: "state", state: {} });
  ports[1].drop();
  assert.equal(bridge.problem(), "retrying");
  bridge.stop();
});

test("the Linux host's wording is understood too, with either apostrophe", () => {
  for (const msg of ["Kairos Desktop is not running.", "Kairos Desktop is not running", "Kairos isn't running.", "Kairos isn\u2019t running."]) {
    assert.ok(saysNotRunning(msg), msg);
  }
  assert.ok(!saysNotRunning("Timed out waiting for Kairos Desktop"));
  assert.ok(!saysNotRunning(undefined));
  const { bridge, ports } = problemBridge({ detail: "Native host has exited." });
  ports[0].receive({ type: "error", message: "Kairos isn't running." });
  ports[0].drop();
  assert.equal(bridge.problem(), "app-not-running");
  bridge.stop();
});

function fakeAlarms() {
  const listeners = [];
  const a = {
    created: {},
    create(name, info) { a.created[name] = info; },
    clear(name) { delete a.created[name]; return Promise.resolve(true); },
    onAlarm: { addListener: (fn) => listeners.push(fn) },
    fire: (name) => listeners.forEach((fn) => fn({ name })),
  };
  return a;
}

test("while disconnected an alarm is set to reconnect, since a sleeping worker loses its timers", () => {
  const ports = [];
  const alarms = fakeAlarms();
  const bridge = createBridge({
    client: "read",
    version: "1",
    alarms,
    connect: () => { const p = fakePort(); ports.push(p); return p; },
    setTimeout: (fn, ms) => (ms >= 1000 ? 0 : setTimeout(fn, ms)), // the backoff timer never fires: the worker slept
  });
  bridge.start();
  ports[0].receive({ type: "state", state: {} });
  assert.deepEqual(alarms.created, {}, "no alarm while connected");
  ports[0].drop();
  assert.ok(alarms.created[RECONNECT_ALARM], "alarm set on disconnect");
  assert.ok(alarms.created[RECONNECT_ALARM].periodInMinutes >= 0.5, "repeats until the app is back");
  alarms.fire("something-else");
  assert.equal(ports.length, 1);
  alarms.fire(RECONNECT_ALARM);
  assert.equal(ports.length, 2, "the alarm reconnects");
  alarms.fire(RECONNECT_ALARM);
  assert.equal(ports.length, 2, "not while an attempt is under way");
  ports[1].receive({ type: "state", state: {} });
  assert.equal(bridge.isConnected(), true);
  assert.deepEqual(alarms.created, {}, "cleared once connected");
  ports[1].drop();
  bridge.stop();
  assert.deepEqual(alarms.created, {}, "cleared when stopped");
});

test("describeConnection tells the user what to do for each problem", () => {
  assert.match(describeConnection(false, "app-not-running").text, /not running/);
  assert.match(describeConnection(false, "app-not-running").hint, /Open Kairos Desktop/);
  assert.match(describeConnection(false, "bridge-missing").text, /bridge is not installed/);
  assert.match(describeConnection(false, "bridge-missing").hint, /Install bridge/);
  assert.match(describeConnection(false, "retrying").text, /retrying/);
  assert.equal(describeConnection(true, null).text, "Connected");
});

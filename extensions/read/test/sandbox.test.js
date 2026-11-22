import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const SOURCE = readFileSync(new URL("../sandbox.js", import.meta.url), "utf8");
const RTC = ["RTCPeerConnection", "webkitRTCPeerConnection", "RTCDataChannel"];

/** Runs sandbox.js against a fake window and document, the way sandbox.html loads it. */
function loadSandbox(extra = {}) {
  const listeners = {};
  const writes = [];
  const win = {
    parent: { postMessage() {} },
    addEventListener: (type, fn) => { listeners[type] = fn; },
    removeEventListener: () => {},
    ...extra,
  };
  for (const name of RTC) win[name] = function Real() {};
  const doc = {
    open() {},
    close() {},
    write(html) { writes.push({ html, rtc: RTC.map((n) => win[n]) }); },
    addEventListener() {},
    querySelectorAll: () => [],
  };
  win.document = doc;
  new Function("window", "document", "URL", "Blob", SOURCE)(win, doc, URL, Blob);
  return { win, writes, load: (html) => listeners.message({ data: { type: "kairos:load", html } }) };
}

test("WebRTC is gone from the sandbox before the scene is written", () => {
  const { win, writes, load } = loadSandbox();
  for (const name of RTC) assert.equal(win[name], undefined, name);
  load("<script>new RTCPeerConnection()</script>");
  assert.deepEqual(writes[0].rtc, [undefined, undefined, undefined]);
});

test("scene code cannot put WebRTC back", () => {
  const { win } = loadSandbox();
  const fake = function Restored() {};
  for (const name of RTC) {
    assert.throws(() => { win[name] = fake; }, TypeError, `assigning ${name}`);
    assert.throws(() => Object.defineProperty(win, name, { value: fake }), TypeError, `redefining ${name}`);
    assert.equal(Reflect.deleteProperty(win, name), false, `deleting ${name}`);
    assert.equal(win[name], undefined);
  }
});

test("a frame's fresh window is locked as soon as script inserts it", () => {
  const child = {};
  for (const name of RTC) child[name] = function Real() {};
  class Node {}
  const win = {};
  Node.prototype.appendChild = function () {
    win.length = 1;
    win[0] = child;
    return this;
  };
  const sandbox = loadSandbox({ Node });
  Object.defineProperty(sandbox.win, "length", { get: () => win.length || 0 });
  Object.defineProperty(sandbox.win, 0, { get: () => win[0] });
  sandbox.win.Node.prototype.appendChild.call({}, {});
  for (const name of RTC) assert.equal(child[name], undefined, name);
  assert.throws(() => { sandbox.win.Node.prototype.appendChild = function () {}; }, TypeError, "the wrapper stays");
});

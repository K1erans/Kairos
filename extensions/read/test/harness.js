// Runs background.js under node against a fake `chrome` that behaves like Chrome where it matters:
// chrome.tts fires "interrupted" on the speaking utterance when it is stopped or replaced, and tts
// events arrive asynchronously. The native port is a fake Kairos Desktop the test scripts.
let instance = 0;

export const tick = (ms = 0) => new Promise((r) => realSetTimeout(r, ms));
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;
const realSetInterval = globalThis.setInterval;
const realClearInterval = globalThis.clearInterval;

function events() {
  const fns = [];
  return { addListener: (fn) => fns.push(fn), fire: (...args) => fns.map((fn) => fn(...args)), fns };
}

export async function startRead({ storage = {}, app = null, tabs = [], windows = [], fetch: fakeFetch } = {}) {
  const timers = new Set();
  globalThis.setTimeout = (fn, ms, ...a) => { const t = realSetTimeout(() => { timers.delete(t); fn(...a); }, ms); timers.add(t); return t; };
  globalThis.clearTimeout = (t) => { timers.delete(t); realClearTimeout(t); };
  globalThis.setInterval = (fn, ms, ...a) => { const t = realSetInterval(fn, ms, ...a); timers.add(t); return t; };
  globalThis.clearInterval = (t) => { timers.delete(t); realClearInterval(t); };

  const local = structuredClone(storage.local || {});
  const session = structuredClone(storage.session || {});
  const area = (data) => ({
    data,
    get: async (keys) => {
      const list = keys == null ? Object.keys(data) : Array.isArray(keys) ? keys : [keys];
      const out = {};
      for (const k of list) if (k in data) out[k] = structuredClone(data[k]);
      return out;
    },
    // Real storage answers after an IPC round trip, so writes resolve on a later task.
    set: (obj) => new Promise((r) => { for (const [k, v] of Object.entries(obj)) data[k] = structuredClone(v); realSetTimeout(r, 1); }),
    remove: async (keys) => { for (const k of [].concat(keys)) delete data[k]; },
  });

  // ---- chrome.tts ----
  const spoken = []; // every utterance handed to chrome.tts.speak
  let current = null;
  const fireLater = (u, type, extra = {}) => realSetTimeout(() => u.options.onEvent && u.options.onEvent({ type, ...extra }), 0);
  const tts = {
    speak(text, options = {}, callback) {
      if (current && !options.enqueue) fireLater(current, "interrupted");
      current = { text, options };
      spoken.push(current);
      fireLater(current, "start");
      if (callback) realSetTimeout(callback, 0);
    },
    stop() {
      if (current) fireLater(current, "interrupted");
      current = null;
    },
    getVoices: (cb) => cb([]),
  };

  // ---- native port (fake Kairos Desktop) ----
  const toApp = [];
  let port = null;
  const makePort = () => {
    const onMessage = events();
    const onDisconnect = events();
    const p = {
      onMessage, onDisconnect,
      postMessage(msg) {
        if (p.closed) throw new Error("Attempting to use a disconnected port object");
        toApp.push(msg);
        if (app) realSetTimeout(() => app(msg, (reply) => !p.closed && onMessage.fire(reply)), 0);
      },
      disconnect() { p.closed = true; },
      closed: false,
    };
    return p;
  };

  // Chrome allows one offscreen document; creating a second throws.
  const offscreen = { open: false, created: 0, errors: [] };
  const sentToTabs = [];
  const broadcasts = [];
  const runtimeOnMessage = events();
  const tabsOnUpdated = events();
  const chrome = {
    runtime: {
      lastError: undefined,
      getManifest: () => ({ version: "0.0.0-test" }),
      getURL: (p) => `chrome-extension://test/${p}`,
      connectNative: () => { port = app ? makePort() : null; if (!port) throw new Error("no host"); return port; },
      sendMessage: async (msg) => {
        if (msg.target === "offscreen") {
          if (!offscreen.open) throw new Error("Could not establish connection. Receiving end does not exist.");
          await tick(5);
          const text = String(msg.html).replace(/<[^>]+>/g, " ").trim();
          return text ? { ok: true, article: { title: "Parsed", paragraphs: [text] } } : { ok: false };
        }
        broadcasts.push(msg);
        return undefined;
      },
      onMessage: runtimeOnMessage,
      onInstalled: events(),
      onStartup: events(),
      getContexts: async () => (offscreen.open ? [{ contextType: "OFFSCREEN_DOCUMENT" }] : []),
      getPlatformInfo: (cb) => cb && cb({}),
    },
    storage: { local: area(local), session: area(session) },
    tts,
    tabs: {
      query: async (q) => tabs.filter((t) => (!q.active || t.active)),
      get: async (id) => tabs.find((t) => t.id === id),
      sendMessage: async (tabId, msg) => {
        sentToTabs.push({ tabId, msg });
        if (msg.type === "kr.ping") return { ok: true };
        if (msg.type === "kr.extract") {
          const tab = tabs.find((t) => t.id === tabId);
          return tab && tab.article ? { ok: true, article: tab.article, url: tab.url } : { ok: false };
        }
        return undefined;
      },
      onUpdated: tabsOnUpdated,
      onRemoved: events(),
      onActivated: events(),
    },
    windows: { get: async (id) => windows.find((w) => w.id === id) || { id, focused: false } },
    scripting: { executeScript: async () => [] },
    sidePanel: { setPanelBehavior: async () => {}, open: async () => {} },
    contextMenus: { removeAll: (cb) => cb && cb(), create: () => {}, onClicked: events() },
    commands: { onCommand: events() },
    offscreen: {
      createDocument: async () => {
        if (offscreen.open) {
          const err = new Error("Only a single offscreen document may be created.");
          offscreen.errors.push(err.message);
          throw err;
        }
        offscreen.open = true;
        offscreen.created++;
      },
      closeDocument: async () => { await tick(3); offscreen.open = false; },
    },
  };
  globalThis.chrome = chrome;
  if (fakeFetch) globalThis.fetch = fakeFetch;

  await import(`../background.js?i=${++instance}`);
  await tick(5);

  const h = {
    chrome, local, session, spoken, toApp, sentToTabs, broadcasts, tabsOnUpdated, offscreen,
    get speaking() { return current; },
    /** Sends a runtime message the way the side panel does and resolves with the reply. */
    send(msg, sender = {}) {
      return new Promise((resolve) => {
        const handled = runtimeOnMessage.fns.some((fn) => fn(msg, sender, resolve) === true);
        if (!handled) resolve(undefined);
      });
    },
    cmd: (cmd, arg) => h.send({ type: "player.cmd", cmd, arg }),
    /** The current utterance finishes normally. */
    async endUtterance() {
      const u = current;
      current = null;
      u.options.onEvent({ type: "end" });
      await tick(5);
    },
    /** Something outside Kairos Read (another extension, the OS) cuts the utterance off. */
    async interruptFromOutside() {
      const u = current;
      current = null;
      u.options.onEvent({ type: "interrupted" });
      await tick(5);
    },
    /** Pushes an app message (state, read.command) over the native port. */
    async push(msg) { port.onMessage.fire(msg); await tick(5); },
    async dropPort() { port.closed = true; port.onDisconnect.fire(); await tick(5); },
    statuses: () => toApp.filter((m) => m.type === "read.status").map((m) => m.state),
    logs: () => toApp.filter((m) => m.type === "read.log"),
    lastPlayer: () => [...broadcasts].reverse().find((m) => m.type === "player.state")?.player,
    dispose() {
      for (const t of timers) { realClearTimeout(t); realClearInterval(t); }
      globalThis.setTimeout = realSetTimeout;
      globalThis.clearTimeout = realClearTimeout;
      globalThis.setInterval = realSetInterval;
      globalThis.clearInterval = realClearInterval;
    },
  };
  return h;
}

export function articleTab(id, url, sentences, extra = {}) {
  return { id, url, title: `Tab ${id}`, active: true, windowId: 1, status: "complete", article: { title: `Article ${id}`, paragraphs: [sentences.join(" ")] }, ...extra };
}

export const html = (text) => new Response(`<html><body><p>${text}</p></body></html>`, { headers: { "content-type": "text/html" } });

/** A fake Kairos Desktop that answers hello with a state and records everything else. */
export function fakeApp(state = { mode: "off" }, extra = () => {}) {
  return (msg, reply) => {
    if (msg.type === "hello" || msg.type === "getState") reply({ type: "state", state });
    else extra(msg, reply);
  };
}

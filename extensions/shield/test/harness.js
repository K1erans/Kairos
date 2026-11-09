// Runs background.js under node against a fake `chrome`. The native port is a fake Kairos Desktop
// the test scripts; storage writes resolve on a later task, as Chrome's do.
let instance = 0;
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;
export const tick = (ms = 0) => new Promise((r) => realSetTimeout(r, ms));

function events() {
  const fns = [];
  return { addListener: (fn) => fns.push(fn), fire: (...args) => fns.map((fn) => fn(...args)), fns };
}

export async function startShield({ app = null, storage = {}, tabs = [] } = {}) {
  const timers = new Set();
  globalThis.setTimeout = (fn, ms, ...a) => { const t = realSetTimeout(() => { timers.delete(t); fn(...a); }, ms); timers.add(t); return t; };
  globalThis.clearTimeout = (t) => { timers.delete(t); realClearTimeout(t); };
  const local = structuredClone(storage.local || {});
  const toApp = [];
  let port = null;
  let appHandler = app;
  const connectNative = () => {
    if (!appHandler) throw new Error("no host");
    const onMessage = events();
    const onDisconnect = events();
    const p = {
      onMessage, onDisconnect, closed: false,
      postMessage(msg) {
        if (p.closed) throw new Error("disconnected port");
        toApp.push(msg);
        const handler = appHandler;
        realSetTimeout(() => handler && handler(msg, (reply) => !p.closed && onMessage.fire(reply)), 0);
      },
      disconnect() { p.closed = true; },
    };
    port = p;
    return p;
  };
  const runtimeOnMessage = events();
  const toTabs = [];
  const dnr = { rules: [] };
  globalThis.chrome = {
    runtime: {
      lastError: undefined,
      getManifest: () => ({ version: "0.0.0-test" }),
      getURL: (p) => `chrome-extension://shield/${p}`,
      connectNative,
      sendMessage: async () => undefined,
      onMessage: runtimeOnMessage,
      onInstalled: events(),
      onStartup: events(),
    },
    storage: {
      local: {
        get: async (keys) => {
          const out = {};
          for (const k of [].concat(keys)) if (k in local) out[k] = structuredClone(local[k]);
          return out;
        },
        set: (obj) => new Promise((r) => { for (const [k, v] of Object.entries(obj)) local[k] = structuredClone(v); realSetTimeout(r, 1); }),
      },
    },
    declarativeNetRequest: {
      getDynamicRules: async () => dnr.rules,
      updateDynamicRules: async ({ addRules }) => { dnr.rules = addRules; },
    },
    alarms: { create: () => {}, clear: () => {}, onAlarm: events() },
    action: { setBadgeText: () => {}, setBadgeBackgroundColor: () => {}, setTitle: () => {} },
    tabs: {
      query: async () => tabs,
      update: async () => {},
      sendMessage: async (tabId, msg) => { toTabs.push({ tabId, msg }); },
      onUpdated: events(),
      goBack: async () => {},
      remove: async () => {},
    },
  };
  await import(`../background.js?i=${++instance}`);
  await tick(10);
  const h = {
    toApp, toTabs, local, dnr,
    send(msg, sender = { tab: { id: 1, incognito: false } }) {
      return new Promise((resolve) => {
        const handled = runtimeOnMessage.fns.some((fn) => fn(msg, sender, resolve) === true);
        if (!handled) resolve(undefined);
      });
    },
    async push(state) { port.onMessage.fire({ type: "state", state }); await tick(10); },
    async drop() { appHandler = null; port.closed = true; port.onDisconnect.fire(); await tick(10); },
    /** Brings the fake app back and fires the pending reconnect. */
    async reconnect(handler) {
      appHandler = handler;
      await new Promise((r) => realSetTimeout(r, 1100));
      await tick(10);
    },
    sent: (type) => toApp.filter((m) => m.type === type),
    dispose() {
      for (const t of timers) realClearTimeout(t);
      globalThis.setTimeout = realSetTimeout;
      globalThis.clearTimeout = realClearTimeout;
    },
  };
  return h;
}

export function fakeApp(getState, extra = () => {}) {
  return (msg, reply) => {
    if (msg.type === "hello" || msg.type === "getState") reply({ type: "state", state: getState() });
    else extra(msg, reply);
  };
}

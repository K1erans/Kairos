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

/** timeScale shrinks the worker's timers (Jev's wait) so a test need not sit through them. */
export async function startShield({ app = null, storage = {}, tabs = [], timeScale = 1 } = {}) {
  const timers = new Set();
  globalThis.setTimeout = (fn, ms, ...a) => { const t = realSetTimeout(() => { timers.delete(t); fn(...a); }, (ms || 0) * timeScale); timers.add(t); return t; };
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
  const tabUpdates = [];
  const wentBack = [];
  const removed = [];
  const alarms = { created: {}, cleared: [] };
  const onAlarm = events();
  const tabsOnUpdated = events();
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
    alarms: {
      create: (name, info) => { alarms.created[name] = info; },
      clear: async (name) => { alarms.cleared.push(name); delete alarms.created[name]; return true; },
      onAlarm,
    },
    action: { setBadgeText: () => {}, setBadgeBackgroundColor: () => {}, setTitle: () => {} },
    tabs: {
      query: async () => tabs,
      update: async (tabId, props) => { tabUpdates.push({ tabId, ...props }); },
      sendMessage: async (tabId, msg) => { toTabs.push({ tabId, msg }); },
      onUpdated: tabsOnUpdated,
      goBack: async (tabId) => { wentBack.push(tabId); },
      remove: async (tabId) => { removed.push(tabId); },
    },
  };
  await import(`../background.js?i=${++instance}`);
  await tick(10);
  const h = {
    toApp, toTabs, tabUpdates, wentBack, removed, local, dnr, alarms,
    fireAlarm: async (name) => { onAlarm.fire({ name }); await tick(10); },
    tabsOnUpdated,
    send(msg, sender = { tab: { id: 1, incognito: false }, frameId: 0 }) {
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

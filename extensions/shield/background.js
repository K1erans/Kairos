// Kairos Shield service worker: holds the focus state, turns it into blocking rules,
// and relays Jev checks and overrides to Kairos Desktop.
import { createBridge, describeConnection } from "./lib/bridge.js";
import { EMPTY_STATE, hostnameOf, normalizeDomain, ruleLockText, sanitizeState, siteVerdict, strictLockUntil } from "./lib/match.js";
import {
  JEV_TIMEOUT_MS,
  buildDnrRules,
  canOverride,
  effectiveState,
  isSafeSelector,
  jevCacheKey,
  jevSkipReason,
  nextExpiry,
  pruneAllows,
  pruneJevCache,
  rememberJev,
  selectorsForHost,
  withSite,
  withoutElement,
} from "./lib/rules.js";

const VERSION = chrome.runtime.getManifest().version;
const BLOCKED_PAGE = chrome.runtime.getURL("blocked.html");
const EXPIRY_ALARM = "kairos-temp-allow-expiry";

let appState = sanitizeState(EMPTY_STATE); // last state pushed by Kairos Desktop
let manualMode = null; // used only while the app is disconnected
let localAllows = {}; // hostname -> expiry, set when an override is granted
let localElements = {}; // hostname -> [selectors] picked in this browser
let connected = false;
let problem = null; // why the app is not reachable; see describeConnection

// The worker is restarted often; everything it needs is reloaded from storage first.
const ready = chrome.storage.local
  .get(["state", "manualMode", "localAllows", "localElements", "outbox"])
  .then((stored) => {
    appState = sanitizeState(stored.state || EMPTY_STATE);
    manualMode = stored.manualMode || null;
    localAllows = pruneAllows(stored.localAllows);
    localElements = stored.localElements || {};
    // Element rules picked while Kairos Desktop was away, from before this worker restarted.
    for (const msg of Array.isArray(stored.outbox) ? stored.outbox : []) bridge.send(msg, { queue: true });
  })
  .catch(() => {});

function current() {
  return effectiveState(appState, { connected, manualMode, localAllows });
}

const bridge = createBridge({
  client: "shield",
  version: VERSION,
  onMessage: (msg) => {
    if (msg.type === "state") {
      ready.then(() => {
        appState = sanitizeState(msg.state);
        chrome.storage.local.set({ state: appState });
        applyState();
      });
    }
  },
  onStatus: (value) => {
    connected = value;
    ready.then(applyState);
  },
  onProblem: (value) => {
    problem = value;
    ready.then(() => updateBadge(current()));
  },
  onOutbox: (outbox) => chrome.storage.local.set({ outbox }),
});
bridge.start();

// ---- applying state -------------------------------------------------------------------------

let applyChain = Promise.resolve();
function applyState() {
  // Serialised so two quick pushes cannot interleave their rule updates.
  applyChain = applyChain.then(doApplyState, doApplyState);
  return applyChain;
}

async function doApplyState() {
  const state = current();
  const rules = buildDnrRules(state, { blockedPageUrl: BLOCKED_PAGE });
  try {
    const existing = await chrome.declarativeNetRequest.getDynamicRules();
    await chrome.declarativeNetRequest.updateDynamicRules({
      removeRuleIds: existing.map((r) => r.id),
      addRules: rules,
    });
  } catch (err) {
    console.error("Kairos Shield: could not update blocking rules", err);
  }

  // The earlier of the next temporary allow ending and a strict lock ending, while the lock decides the mode.
  const lock = connected ? null : strictLockUntil(appState);
  const expiries = [nextExpiry(state.rules.temporaryAllows), lock].filter((t) => t !== null);
  const expiry = expiries.length ? Math.min(...expiries) : null;
  if (expiry) chrome.alarms.create(EXPIRY_ALARM, { when: expiry + 500 });
  else chrome.alarms.clear(EXPIRY_ALARM);

  updateBadge(state);

  // Tabs that were already open: redirect the ones now blocked, refresh the rest.
  let tabs = [];
  try {
    tabs = await chrome.tabs.query({});
  } catch (_) {
    /* no tabs permission yet */
  }
  for (const tab of tabs) {
    if (!tab.id || !tab.url) continue;
    if (/^https?:\/\//i.test(tab.url) && siteVerdict(tab.url, state) === "blocked") {
      chrome.tabs.update(tab.id, { url: `${BLOCKED_PAGE}#${tab.url}` }).catch(() => {});
    } else {
      chrome.tabs.sendMessage(tab.id, { type: "page.update" }).catch(() => {});
    }
  }
  chrome.runtime.sendMessage({ type: "page.update" }).catch(() => {});
}

function updateBadge(state) {
  if (!connected) {
    chrome.action.setBadgeText({ text: "!" });
    chrome.action.setBadgeBackgroundColor({ color: "#D97706" });
    chrome.action.setTitle({ title: `Kairos Shield: ${describeConnection(false, problem).text} Using the last known mode.` });
    return;
  }
  const label = { focus: "F", strict: "S", break: "B", off: "" }[state.mode] || "";
  chrome.action.setBadgeText({ text: label });
  chrome.action.setBadgeBackgroundColor({ color: state.mode === "strict" ? "#B91C1C" : "#4F7CFF" });
  chrome.action.setTitle({ title: `Kairos Shield: ${state.mode}` });
}

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name !== EXPIRY_ALARM) return;
  ready.then(() => {
    localAllows = pruneAllows(localAllows);
    chrome.storage.local.set({ localAllows });
    applyState();
  });
});

chrome.runtime.onInstalled.addListener(() => ready.then(applyState));
chrome.runtime.onStartup.addListener(() => ready.then(applyState));

// Single-page apps change URL without a load; tell the page so it re-runs its checks.
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.url) chrome.tabs.sendMessage(tabId, { type: "page.navigated", url: changeInfo.url }).catch(() => {});
});

// ---- messages from pages and the popup ------------------------------------------------------

function pageInfo(url, incognito = false) {
  const state = current();
  const host = hostnameOf(url);
  return {
    jev: !jevSkipReason(url, state, { connected, incognito }),
    mode: state.mode,
    connected,
    groupName: state.groupName,
    groupColor: state.groupColor,
    task: state.currentTask ? state.currentTask.title : "",
    endsAt: state.endsAt,
    overridesLeft: state.overridesLeft,
    overrideMinutes: state.overrideMinutes,
    canOverride: canOverride(state) && (!connected || state.overridesLeft > 0),
    verdict: siteVerdict(url, state),
    selectors: selectorsForHost(host, state, localElements),
  };
}

async function jevCheck(page, sender) {
  const state = current();
  const incognito = !!(sender.tab && sender.tab.incognito);
  if (jevSkipReason(page.url, state, { connected, incognito })) return { block: false };

  const key = jevCacheKey(page.url, state);
  const stored = await chrome.storage.local.get("jevCache");
  let cache = pruneJevCache(stored.jevCache);
  let block;
  if (cache[key]) {
    block = cache[key].block === true;
  } else {
    try {
      const result = await bridge.request(
        { type: "jev.check", url: page.url, title: page.title || "", description: page.description || "", excerpt: page.excerpt || "" },
        JEV_TIMEOUT_MS,
      );
      block = result.block === true;
      cache = rememberJev(cache, key, result);
      chrome.storage.local.set({ jevCache: cache });
    } catch (_) {
      return { block: false }; // slow or offline: treated as "unsure" and let through
    }
  }
  if (!block) return { block: false };
  // The state may have changed while Jev was answering; the page asks again after page.update.
  const now = current();
  if (jevSkipReason(page.url, now, { connected, incognito }) || jevCacheKey(page.url, now) !== key) return { block: false };
  bridge.send({ type: "blockEvent", target: page.url, layer: "jev", overridden: false });
  return { block: true, ...pageInfo(page.url, incognito) };
}

async function requestOverride(target, reason) {
  const state = current();
  const host = normalizeDomain(target);
  if (!host) return { allowed: false, message: "Unknown site." };
  if (!reason || !reason.trim()) return { allowed: false, message: "A reason is required." };
  if (!canOverride(state)) return { allowed: false, message: "Overrides are off until this session ends." };

  let result;
  if (connected) {
    try {
      result = await bridge.request({ type: "override.request", target: host, reason: reason.trim() }, 10000);
    } catch (err) {
      return { allowed: false, message: err.message };
    }
  } else {
    // Kairos Desktop is away, so nothing is counting overrides; allow the length it last reported locally.
    result = { allowed: true, until: Date.now() + state.overrideMinutes * 60_000, remaining: 0 };
  }
  if (result.allowed) {
    const until = typeof result.until === "number" ? result.until : Date.now() + state.overrideMinutes * 60_000;
    localAllows = { ...pruneAllows(localAllows), [host]: until };
    await chrome.storage.local.set({ localAllows });
    await applyState();
    return { allowed: true, until, remaining: result.remaining };
  }
  return { allowed: false, remaining: result.remaining, message: result.message || "Not allowed." };
}

async function goBack(sender) {
  const tabId = sender.tab && sender.tab.id;
  if (!tabId) return {};
  try {
    await chrome.tabs.goBack(tabId);
  } catch (_) {
    // Nothing to go back to: this tab was opened on the blocked page.
    await chrome.tabs.remove(tabId).catch(() => {});
  }
  return {};
}

async function addElementRule(host, selector) {
  const domain = normalizeDomain(host);
  if (!domain || !isSafeSelector(selector)) return { ok: false };
  const list = new Set(localElements[domain] || []);
  list.add(selector.trim());
  localElements = { ...localElements, [domain]: [...list] };
  await chrome.storage.local.set({ localElements });
  bridge.send({ type: "rule.addElement", host: domain, selector: selector.trim() }, { queue: true });
  await applyState();
  return { ok: true };
}

async function addSiteRule(host, list) {
  const site = normalizeDomain(host);
  if (!site || (list !== "block" && list !== "allow")) return { ok: false, message: "That is not a site." };
  // An allow-for-good would undo what strict mode promises: no way out until the session ends.
  if (list === "allow" && current().mode === "strict") return { ok: false, message: "Strict mode: sites cannot be allowed until the session ends." };
  appState = { ...appState, rules: withSite(appState.rules, site, list) };
  if (list === "block") localAllows = Object.fromEntries(Object.entries(localAllows).filter(([h]) => normalizeDomain(h) !== site));
  await chrome.storage.local.set({ state: appState, localAllows });
  bridge.send({ type: "rule.addSite", site, list }, { queue: true });
  await applyState();
  return { ok: true, site, list };
}

async function removeElementRule(host, selector) {
  const domain = normalizeDomain(host);
  if (!domain || typeof selector !== "string") return { ok: false };
  // Showing an element again loosens the rules, which a locked strict phase does not allow
  // (Kairos Desktop refuses rule.removeElement until then too).
  const locked = strictLockUntil(appState);
  if (locked) return { ok: false, lockedUntil: locked, message: ruleLockText(locked) };
  // The app keeps its own copy and pushes it back in hiddenElements, so it has to forget it too.
  // Until it pushes a state without it, drop it from the copy held here so the page updates now.
  localElements = withoutElement(localElements, domain, selector);
  appState = { ...appState, rules: { ...appState.rules, hiddenElements: withoutElement(appState.rules.hiddenElements, domain, selector) } };
  await chrome.storage.local.set({ localElements, state: appState });
  bridge.send({ type: "rule.removeElement", host: domain, selector }, { queue: true });
  await applyState();
  return { ok: true };
}

async function handle(msg, sender) {
  await ready;
  switch (msg.type) {
    case "page.query":
      return pageInfo(msg.url || (sender.tab && sender.tab.url) || "", !!(sender.tab && sender.tab.incognito));
    case "jev.page":
      return jevCheck(msg, sender);
    case "override":
      return requestOverride(msg.target, msg.reason);
    case "blocked.shown":
      // blocked.html is web accessible, so any site can open it with any URL: log real blocks only.
      if (siteVerdict(msg.url || "", current()) !== "blocked") return {};
      bridge.send({ type: "blockEvent", target: hostnameOf(msg.url) || msg.url, layer: "rule", overridden: false });
      return {};
    case "goBack":
      return goBack(sender);
    case "picker.picked":
      return addElementRule(msg.host, msg.selector);
    case "site.add":
      return addSiteRule(msg.host, msg.list);
    case "elements.remove":
      return removeElementRule(msg.host, msg.selector);
    case "popup.get": {
      const state = current();
      const host = hostnameOf(msg.url || "");
      return {
        state,
        connected,
        connection: describeConnection(connected, problem),
        manualMode,
        lockedUntil: connected ? null : strictLockUntil(appState),
        rulesLockedUntil: strictLockUntil(appState),
        host,
        site: normalizeDomain(host),
        verdict: siteVerdict(msg.url || "", state),
        localSelectors: host ? Object.entries(localElements).filter(([d]) => host === d || host.endsWith("." + d)).flatMap(([d, list]) => list.map((selector) => ({ host: d, selector }))) : [],
      };
    }
    case "popup.setMode":
      if (connected) return { ok: false };
      if (strictLockUntil(appState)) return { ok: false, lockedUntil: strictLockUntil(appState) };
      manualMode = ["off", "focus", "break", "strict"].includes(msg.mode) ? msg.mode : null;
      await chrome.storage.local.set({ manualMode });
      await applyState();
      return { ok: true };
    default:
      return undefined;
  }
}

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || typeof msg.type !== "string" || msg.type === "page.update") return false;
  handle(msg, sender).then(sendResponse, (err) => sendResponse({ error: String(err && err.message) }));
  return true;
});

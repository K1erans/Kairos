// Kairos Shield service worker: holds the focus state, turns it into blocking rules,
// and relays Jev checks and overrides to Kairos Desktop.
import { createBridge, describeConnection } from "./lib/bridge.js";
import { EMPTY_STATE, hostnameOf, normalizeDomain, ruleLockText, sanitizeState, siteVerdict, strictLockUntil } from "./lib/match.js";
import {
  JEV_LATE_MS,
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
const OWN_ORIGIN = chrome.runtime.getURL("");
const EXPIRY_ALARM = "kairos-temp-allow-expiry";

let appState = sanitizeState(EMPTY_STATE); // last state pushed by Kairos Desktop
let manualMode = null; // chosen in the popup while the app is away; cleared when it comes back
let localAllows = {}; // hostname -> expiry, set when an override is granted; dropped on every app state
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
        // The app's temporaryAllows hold every override it granted; a grant made here (while it
        // was away, or before this push) must not outlive a new mode or session.
        localAllows = {};
        chrome.storage.local.set({ state: appState, localAllows });
        applyState();
      });
    }
  },
  onStatus: (value) => {
    connected = value;
    ready.then(() => {
      // The popup's switch only stands in for the app while it is away. Kept, it would come back
      // at the next disconnect or browser start and could switch blocking off.
      if (value && manualMode !== null) {
        manualMode = null;
        chrome.storage.local.set({ manualMode: null });
      }
      return applyState();
    });
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

// Single-page apps change URL without a load; tell the page so it re-runs its checks. A tab that
// lands on a blocked site without a network request the rules saw (Back into the page cache, or a
// load that began before the rules changed) goes to the block page.
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (!changeInfo.url) return;
  ready.then(() => {
    if (/^https?:\/\//i.test(changeInfo.url) && siteVerdict(changeInfo.url, current()) === "blocked") {
      chrome.tabs.update(tabId, { url: `${BLOCKED_PAGE}#${changeInfo.url}` }).catch(() => {});
    } else {
      chrome.tabs.sendMessage(tabId, { type: "page.navigated", url: changeInfo.url }).catch(() => {});
    }
  });
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

const jevAsking = new Map(); // cache key -> the jev.check under way for it
let jevCacheChain = Promise.resolve();

/** One jev.check per page at a time; its answer is cached when it comes, however late. */
function askJev(page, key) {
  let answer = jevAsking.get(key);
  if (answer) return answer;
  answer = bridge
    .request(
      { type: "jev.check", url: page.url, title: page.title || "", description: page.description || "", excerpt: page.excerpt || "" },
      JEV_TIMEOUT_MS + JEV_LATE_MS,
    )
    .then((result) => {
      jevCacheChain = jevCacheChain
        .then(async () => {
          const stored = await chrome.storage.local.get("jevCache");
          await chrome.storage.local.set({ jevCache: rememberJev(pruneJevCache(stored.jevCache), key, result) });
        })
        .catch(() => {});
      return result;
    })
    .finally(() => jevAsking.delete(key));
  jevAsking.set(key, answer);
  return answer;
}

/** The reply for a Jev verdict, unless the state changed while Jev was answering. */
function jevVerdict(page, key, block, incognito) {
  if (!block) return { block: false };
  // The page asks again after page.update.
  const now = current();
  if (jevSkipReason(page.url, now, { connected, incognito }) || jevCacheKey(page.url, now) !== key) return { block: false };
  bridge.send({ type: "blockEvent", target: page.url, layer: "jev", overridden: false });
  return { block: true, ...pageInfo(page.url, incognito) };
}

async function jevCheck(page, sender) {
  const state = current();
  const incognito = !!(sender.tab && sender.tab.incognito);
  if (jevSkipReason(page.url, state, { connected, incognito })) return { block: false };

  const key = jevCacheKey(page.url, state);
  const stored = await chrome.storage.local.get("jevCache");
  const cached = pruneJevCache(stored.jevCache)[key];
  if (cached) return jevVerdict(page, key, cached.block === true, incognito);

  const answer = askJev(page, key);
  let timer;
  const outcome = await Promise.race([
    answer.then((result) => ({ result }), () => ({ failed: true })),
    new Promise((resolve) => { timer = setTimeout(() => resolve({ slow: true }), JEV_TIMEOUT_MS); }),
  ]);
  clearTimeout(timer);
  if (outcome.result) return jevVerdict(page, key, outcome.result.block === true, incognito);
  if (outcome.failed) return { block: false }; // the app went away: Jev needs it
  // Kairos Desktop is slow to answer: its "block when Jev is slow or offline" setting decides
  // for now, and the real answer replaces that when it comes.
  const shown = jevVerdict(page, key, state.rules.jevFailClosed === true, incognito);
  answer.then((result) => lateJev(page, key, result, shown.block, sender, incognito), () => {});
  return shown;
}

/** Tells the page about a Jev answer that came after it had been given the fallback. */
function lateJev(page, key, result, shownBlock, sender, incognito) {
  const tabId = sender.tab && sender.tab.id;
  if (!tabId || (result.block === true) === shownBlock) return;
  const verdict = jevVerdict(page, key, result.block === true, incognito);
  if (verdict.block === shownBlock) return;
  const options = typeof sender.frameId === "number" ? { frameId: sender.frameId } : undefined;
  chrome.tabs.sendMessage(tabId, { type: "jev.late", url: page.url, ...verdict }, options).catch(() => {});
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
    // While connected this only bridges the gap until the app pushes the state that holds the grant.
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
  // Only the top frame: blocked.html can be framed by any site (it is web accessible), and its
  // Back would then act on the framing tab. Of this extension's pages only the block page may ask.
  if (sender.frameId !== 0) return {};
  const url = sender.url || "";
  if (url.startsWith(OWN_ORIGIN) && !url.startsWith(BLOCKED_PAGE)) return {};
  try {
    await chrome.tabs.goBack(tabId);
  } catch (_) {
    // Nothing to go back to: this tab was opened on the blocked page.
    await chrome.tabs.remove(tabId).catch(() => {});
  }
  return {};
}

/** The content script saw a blocked verdict on a page that loaded anyway: send the tab to the block page. */
async function redirectBlocked(sender) {
  const tabId = sender.tab && sender.tab.id;
  if (!tabId || sender.frameId !== 0) return { redirected: false };
  const url = sender.url || sender.tab.url || "";
  if (!/^https?:\/\//i.test(url) || siteVerdict(url, current()) !== "blocked") return { redirected: false };
  await chrome.tabs.update(tabId, { url: `${BLOCKED_PAGE}#${url}` }).catch(() => {});
  return { redirected: true };
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
  // This extension's own pages only work at the top of a tab; a framed block page is a site's trick.
  if (sender.tab && typeof sender.frameId === "number" && sender.frameId !== 0 && (sender.url || "").startsWith(OWN_ORIGIN)) return {};
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
    case "page.blocked":
      return redirectBlocked(sender);
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
      // A grant made in focus ends when the mode leaves focus.
      if (current().mode !== "focus") localAllows = {};
      await chrome.storage.local.set({ manualMode, localAllows });
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

// Pure rule logic for Kairos Shield. No chrome.* use, so it runs under node --test.
import {
  activeTemporaryAllows,
  hostMatches,
  hostnameOf,
  modeUsesElementRules,
  modeUsesJev,
  modeUsesSiteRules,
  normalizeDomain,
  matchesAny,
  siteVerdict,
  strictLockUntil,
} from "./match.js";

export const RULE_ID_BLOCK = 1;
export const RULE_ID_ALLOW = 2;
export const RULE_ID_TEMP_ALLOW = 3;
export const JEV_CACHE_MS = 24 * 60 * 60 * 1000;
export const JEV_TIMEOUT_MS = 2000;
export const EXCERPT_WORDS = 500;

/** Built-in element presets, merged with the group's hiddenElements. */
export const PRESETS = Object.freeze({
  "youtube.com": [
    "#related",
    "ytd-watch-next-secondary-results-renderer",
    "ytd-reel-shelf-renderer",
    "ytd-rich-shelf-renderer[is-shorts]",
    "ytd-guide-entry-renderer:has(a[title='Shorts'])",
    "ytd-mini-guide-entry-renderer[aria-label='Shorts']",
    "ytd-browse[page-subtype='home'] ytd-rich-grid-renderer",
    ".ytp-endscreen-content",
  ],
  "x.com": [
    "[aria-label='Timeline: Trending now']",
    "[data-testid='sidebarColumn'] section",
    "[aria-label='Home timeline'] [data-testid='cellInnerDiv']:not(:has([data-testid='tweetTextarea_0']))",
  ],
  "twitter.com": ["[aria-label='Timeline: Trending now']", "[data-testid='sidebarColumn'] section"],
  "reddit.com": ["#right-sidebar-container", "aside[aria-label]", ".side", "shreddit-gallery-carousel"],
  "linkedin.com": [".scaffold-finite-scroll", "main[aria-label='Main Feed']", ".feed-shared-update-v2", "aside.scaffold-layout__aside"],
});

function cleanDomains(entries) {
  const seen = new Set();
  for (const e of entries || []) {
    const d = normalizeDomain(e);
    if (d) seen.add(d);
  }
  return [...seen].sort();
}

/**
 * Dynamic declarativeNetRequest rules for a state.
 * Block = redirect of main-frame loads to the packaged block page with the original URL in the hash.
 * Allow list (priority 2) and unexpired temporary allows (priority 3) outrank the block (priority 1).
 */
export function buildDnrRules(state, { blockedPageUrl, now = Date.now() }) {
  if (!state || !modeUsesSiteRules(state.mode)) return [];
  const rules = [];
  const block = cleanDomains(state.rules.blockSites);
  if (block.length === 0) return [];
  rules.push({
    id: RULE_ID_BLOCK,
    priority: 1,
    action: { type: "redirect", redirect: { regexSubstitution: `${blockedPageUrl}#\\1` } },
    condition: { regexFilter: "^(https?://.+)$", requestDomains: block, resourceTypes: ["main_frame"] },
  });
  const allow = cleanDomains(state.rules.allowSites);
  if (allow.length) {
    rules.push({
      id: RULE_ID_ALLOW,
      priority: 2,
      action: { type: "allow" },
      condition: { requestDomains: allow, resourceTypes: ["main_frame"] },
    });
  }
  const temp = cleanDomains(activeTemporaryAllows(state.rules.temporaryAllows, now));
  if (temp.length) {
    rules.push({
      id: RULE_ID_TEMP_ALLOW,
      priority: 3,
      action: { type: "allow" },
      condition: { requestDomains: temp, resourceTypes: ["main_frame"] },
    });
  }
  return rules;
}

/** Earliest future expiry among temporary allows, or null. */
export function nextExpiry(temporaryAllows, now = Date.now()) {
  let next = null;
  for (const until of Object.values(temporaryAllows || {})) {
    if (typeof until === "number" && until > now && (next === null || until < next)) next = until;
  }
  return next;
}

/** Drops expired entries. */
export function pruneAllows(allows, now = Date.now()) {
  const out = {};
  for (const [host, until] of Object.entries(allows || {})) {
    if (typeof until === "number" && until > now) out[host] = until;
  }
  return out;
}

/**
 * State as the extension enforces it: local allows merged in, and while the app is away the manual
 * mode, unless the last state it sent holds a locked strict phase (strictUntil in the future),
 * which stays strict whatever was chosen.
 */
export function effectiveState(state, { connected, manualMode, localAllows, now = Date.now() }) {
  const merged = { ...pruneAllows(state.rules.temporaryAllows, now) };
  for (const [host, until] of Object.entries(pruneAllows(localAllows, now))) {
    merged[host] = Math.max(merged[host] || 0, until);
  }
  return {
    ...state,
    mode: connected ? state.mode : strictLockUntil(state, now) ? "strict" : manualMode || state.mode,
    rules: { ...state.rules, temporaryAllows: merged },
  };
}

/** Selectors to hide on a hostname: presets + group rules + rules picked locally. */
export function selectorsForHost(hostname, state, localElements = {}) {
  if (!state || !modeUsesElementRules(state.mode) || !hostname) return [];
  const out = new Set();
  for (const source of [PRESETS, state.rules.hiddenElements, localElements]) {
    for (const [domain, selectors] of Object.entries(source || {})) {
      if (!hostMatches(hostname, domain) || !Array.isArray(selectors)) continue;
      for (const s of selectors) if (isSafeSelector(s)) out.add(s.trim());
    }
  }
  return [...out];
}

/** hiddenElements without one selector on a site, under whatever form its key was written. */
export function withoutElement(hiddenElements, host, selector) {
  const domain = normalizeDomain(host);
  const out = {};
  for (const [key, list] of Object.entries(hiddenElements || {})) {
    if (!Array.isArray(list)) continue;
    const kept = normalizeDomain(key) === domain ? list.filter((s) => s !== selector) : list;
    if (kept.length) out[key] = kept;
  }
  return out;
}

/** Rules with `site` on one site list and off the other, and any temporary allow on it ended when blocking. */
export function withSite(rules, site, list) {
  const own = list === "block" ? "blockSites" : "allowSites";
  const other = list === "block" ? "allowSites" : "blockSites";
  const without = (entries) => (entries || []).filter((e) => normalizeDomain(e) !== site);
  const temporaryAllows = Object.fromEntries(Object.entries(rules.temporaryAllows || {}).filter(([h]) => list === "allow" || normalizeDomain(h) !== site));
  return { ...rules, [own]: [...without(rules[own]), site], [other]: without(rules[other]), temporaryAllows };
}

/** Rejects anything that could break out of the generated rule. */
export function isSafeSelector(selector) {
  return typeof selector === "string" && selector.trim().length > 0 && selector.length < 500 && !/[{}<>;@]|\/\*/.test(selector);
}

/** One rule per selector so an invalid selector cannot void the others. */
export function cssForSelectors(selectors) {
  return selectors.map((s) => `${s}{display:none!important}`).join("\n");
}

export function firstWords(text, count = EXCERPT_WORDS) {
  const words = String(text || "").split(/\s+/).filter(Boolean);
  return words.slice(0, count).join(" ");
}

/** A verdict holds only for the group, task and threshold it was made under. */
export function jevCacheKey(url, state) {
  let clean = String(url || "");
  const hash = clean.indexOf("#");
  if (hash >= 0) clean = clean.slice(0, hash);
  const task = state.currentTask && state.currentTask.id ? state.currentTask.id : "none";
  return `${state.activeGroupId || "none"}|${task}|${state.rules.jevThreshold}|${clean}`;
}

/** Cache with a Jev answer added. "unsure" is not an answer worth keeping for a day. */
export function rememberJev(cache, key, result, now = Date.now()) {
  if (!result || result.choice === "unsure") return cache;
  return { ...cache, [key]: { block: result.block === true, choice: result.choice, at: now } };
}

export function pruneJevCache(cache, now = Date.now()) {
  const out = {};
  for (const [key, entry] of Object.entries(cache || {})) {
    if (entry && typeof entry.at === "number" && now - entry.at < JEV_CACHE_MS) out[key] = entry;
  }
  return out;
}

/**
 * Whether a page should go to Jev at all. Returns a reason string when it should not.
 */
export function jevSkipReason(url, state, { connected, incognito, now = Date.now() }) {
  if (incognito) return "incognito";
  if (!connected) return "disconnected";
  if (!state || !modeUsesJev(state.mode)) return "mode";
  if (!state.rules.jevEnabled) return "disabled";
  if (!/^https?:\/\//i.test(url || "")) return "scheme";
  const host = hostnameOf(url);
  if (!host) return "host";
  if (matchesAny(host, state.rules.neverAnalyse)) return "never-analyse";
  if (siteVerdict(url, state, now) !== "unlisted") return "listed";
  return null;
}

/** Original URL carried in the block page's hash (everything after the first "#"). */
export function originalUrlFromHash(hash) {
  const raw = String(hash || "").replace(/^#/, "");
  return /^https?:\/\//i.test(raw) ? raw : "";
}

export function canOverride(state) {
  return !!state && state.mode === "focus";
}

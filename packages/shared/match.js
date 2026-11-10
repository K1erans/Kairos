// Host and rule matching shared by Kairos Shield and Kairos Read. No chrome.* use.
// Source of truth is packages/shared/match.js; scripts/sync-shared.sh copies it into each extension.

/** Normalises a rule entry or URL to a bare lowercase hostname ("" when unusable). */
export function normalizeDomain(entry) {
  if (typeof entry !== "string") return "";
  let s = entry.trim().toLowerCase();
  if (!s) return "";
  s = s.replace(/^[a-z][a-z0-9+.-]*:\/\//, "");
  s = s.split(/[/?#]/)[0];
  s = s.replace(/^.*@/, "").replace(/:\d+$/, "").replace(/^\*\./, "").replace(/^\.+|\.+$/g, "");
  // Hostnames reach us in ASCII (punycode) form, so Unicode entries such as münchen.de are converted.
  if (/[^\x00-\x7f]/.test(s)) {
    try {
      s = new URL(`https://${s}`).hostname;
    } catch (_) {
      return "";
    }
  }
  if (s.startsWith("www.")) s = s.slice(4);
  return /^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(s) ? s : "";
}

export function hostnameOf(url) {
  try {
    return new URL(url).hostname.toLowerCase().replace(/\.$/, "");
  } catch (_) {
    return "";
  }
}

/** A site matches a rule when its hostname equals the entry or ends with "." + entry. */
export function hostMatches(hostname, entry) {
  const rule = normalizeDomain(entry);
  const host = (hostname || "").toLowerCase().replace(/\.$/, "");
  if (!rule || !host) return false;
  return host === rule || host.endsWith("." + rule);
}

export function matchesAny(hostname, entries) {
  return Array.isArray(entries) && entries.some((e) => hostMatches(hostname, e));
}

/** Hostnames with a temporary allow that has not expired. */
export function activeTemporaryAllows(temporaryAllows, now) {
  const out = [];
  for (const [host, until] of Object.entries(temporaryAllows || {})) {
    if (typeof until === "number" && until > now && normalizeDomain(host)) out.push(normalizeDomain(host));
  }
  return out;
}

export function modeUsesSiteRules(mode) {
  return mode === "focus" || mode === "strict";
}
export function modeUsesElementRules(mode) {
  return mode === "focus" || mode === "break" || mode === "strict";
}
export function modeUsesJev(mode) {
  return mode === "focus" || mode === "strict";
}

/**
 * How the site rules treat a URL: "off" (mode has no site rules), "allowed" (allow list or
 * temporary allow), "blocked" (block list) or "unlisted" (on no list; Jev's territory).
 */
export function siteVerdict(url, state, now = Date.now()) {
  if (!state || !modeUsesSiteRules(state.mode)) return "off";
  const host = hostnameOf(url);
  if (!host) return "unlisted";
  const rules = state.rules || {};
  if (matchesAny(host, activeTemporaryAllows(rules.temporaryAllows, now))) return "allowed";
  if (matchesAny(host, rules.allowSites)) return "allowed";
  if (matchesAny(host, rules.blockSites)) return "blocked";
  return "unlisted";
}

export const EMPTY_STATE = Object.freeze({
  mode: "off",
  activeGroupId: null,
  groupName: "",
  groupColor: "#4F7CFF",
  currentTask: null,
  endsAt: null,
  overridesLeft: 0,
  strictUntil: null,
  overrideMinutes: 5,
  cloudVoice: null,
  rules: {
    blockSites: [],
    allowSites: [],
    hiddenElements: {},
    jevEnabled: false,
    jevThreshold: 0.7,
    neverAnalyse: [],
    temporaryAllows: {},
  },
});

/** Fills in missing fields so callers can read state.rules.x without guards. */
export function sanitizeState(raw) {
  const s = raw && typeof raw === "object" ? raw : {};
  const r = s.rules && typeof s.rules === "object" ? s.rules : {};
  const mode = ["off", "focus", "break", "strict"].includes(s.mode) ? s.mode : "off";
  return {
    mode,
    activeGroupId: s.activeGroupId ?? null,
    groupName: typeof s.groupName === "string" ? s.groupName : "",
    groupColor: typeof s.groupColor === "string" ? s.groupColor : "#4F7CFF",
    currentTask: s.currentTask && typeof s.currentTask.title === "string" ? s.currentTask : null,
    endsAt: typeof s.endsAt === "number" ? s.endsAt : null,
    overridesLeft: typeof s.overridesLeft === "number" ? s.overridesLeft : 0,
    overrideMinutes: typeof s.overrideMinutes === "number" && s.overrideMinutes >= 1 && s.overrideMinutes <= 24 * 60 ? Math.round(s.overrideMinutes) : 5,
    strictUntil: typeof s.strictUntil === "number" ? s.strictUntil : null,
    cloudVoice: s.cloudVoice && typeof s.cloudVoice.name === "string" && s.cloudVoice.name ? { provider: String(s.cloudVoice.provider || ""), name: s.cloudVoice.name } : null,
    rules: {
      blockSites: Array.isArray(r.blockSites) ? r.blockSites : [],
      allowSites: Array.isArray(r.allowSites) ? r.allowSites : [],
      hiddenElements: r.hiddenElements && typeof r.hiddenElements === "object" ? r.hiddenElements : {},
      jevEnabled: r.jevEnabled === true,
      jevThreshold: typeof r.jevThreshold === "number" ? r.jevThreshold : 0.7,
      neverAnalyse: Array.isArray(r.neverAnalyse) ? r.neverAnalyse : [],
      temporaryAllows: r.temporaryAllows && typeof r.temporaryAllows === "object" ? r.temporaryAllows : {},
    },
  };
}

/** When a locked strict phase ends (ms), or null when no lock is in force at `now`. See "Limits and conventions" in PROTOCOL.md. */
export function strictLockUntil(state, now = Date.now()) {
  return state && typeof state.strictUntil === "number" && state.strictUntil > now ? state.strictUntil : null;
}

/** Why a rule cannot be loosened now (a locked strict phase ends at `until`, ms). Kairos Desktop says the same on its Rules page. */
export function ruleLockText(until) {
  const at = new Date(until).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return `Strict focus until ${at}: you can block more, but nothing can be unblocked or allowed until then.`;
}

/** "5 minutes" / "1 minute", for the labels that offer an override. */
export function minutesText(minutes) {
  return minutes === 1 ? "1 minute" : `${minutes} minutes`;
}

export function formatTimeLeft(endsAt, now = Date.now()) {
  if (typeof endsAt !== "number") return "";
  const total = Math.max(0, Math.round((endsAt - now) / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

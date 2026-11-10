import { formatTimeLeft, ruleLockText } from "./lib/match.js";

const $ = (id) => document.getElementById(id);
const LABELS = { off: "Off", focus: "Focus", break: "Break", strict: "Strict" };
let tab = null;
let data = null;

async function load() {
  [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  data = await chrome.runtime.sendMessage({ type: "popup.get", url: tab && tab.url ? tab.url : "" }).catch(() => null);
  render();
}

function jevStatus(state, connected) {
  if (!connected) return "Needs Kairos Desktop";
  if (state.mode !== "focus" && state.mode !== "strict") return "Off in this mode";
  return state.rules.jevEnabled ? "On" : "Off in settings";
}

function render() {
  if (!data) return;
  const { state, connected, manualMode } = data;
  $("group").textContent = state.groupName || "Kairos Shield";
  $("dot").style.background = state.mode === "off" ? "#9aa0ad" : state.groupColor;
  $("mode").textContent = LABELS[state.mode] || state.mode;
  $("task").textContent = state.currentTask ? state.currentTask.title : "None";
  $("time").textContent = formatTimeLeft(state.endsAt) || "–";
  $("overrides").textContent = state.mode === "focus" && connected ? String(state.overridesLeft) : "–";
  $("groupRow").textContent = state.groupName || "None";
  $("jev").textContent = jevStatus(state, connected);
  $("bridge").textContent = connected ? "Connected" : "Not connected";
  $("warn").hidden = connected;
  $("warn").textContent = `${data.connection.text} ${data.connection.hint} Shield is using the last known mode.`;
  $("manualRow").hidden = connected;
  $("manual").value = manualMode || "";
  const locked = typeof data.lockedUntil === "number" ? data.lockedUntil : null;
  $("manual").disabled = locked !== null;
  $("lockNote").hidden = locked === null;
  if (locked !== null) {
    const at = new Date(locked).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    $("lockNote").textContent = `Locked in Strict until ${at}. The mode cannot be changed here until then, even while Kairos Desktop is away.`;
  }
  const site = data.site;
  $("siteBox").hidden = !(tab && /^https?:\/\//i.test(tab.url || "")) || !site;
  $("siteName").textContent = site ? `${site}: ${{ blocked: "blocked", allowed: "allowed", unlisted: "not on a list", off: "no site rules in this mode" }[data.verdict] || ""}` : "";
  $("allowSite").disabled = state.mode === "strict";
  $("allowSite").title = state.mode === "strict" ? "Strict mode: sites cannot be allowed until the session ends." : "";
  const pickable = !!(tab && /^https?:\/\//i.test(tab.url || ""));
  $("pick").disabled = !pickable;
  const list = $("list");
  list.textContent = "";
  const rulesLocked = typeof data.rulesLockedUntil === "number" ? data.rulesLockedUntil : null;
  $("rulesNote").hidden = rulesLocked === null;
  $("rulesNote").textContent = rulesLocked === null ? "" : ruleLockText(rulesLocked);
  for (const rule of data.localSelectors || []) {
    const li = document.createElement("li");
    const code = document.createElement("code");
    code.textContent = rule.selector;
    code.title = rule.selector;
    const remove = document.createElement("button");
    remove.textContent = "Show";
    remove.disabled = rulesLocked !== null;
    remove.addEventListener("click", async () => {
      await chrome.runtime.sendMessage({ type: "elements.remove", host: rule.host, selector: rule.selector });
      load();
    });
    li.append(code, remove);
    list.append(li);
  }
  $("rules").hidden = (data.localSelectors || []).length === 0;
}

$("manual").addEventListener("change", async (e) => {
  await chrome.runtime.sendMessage({ type: "popup.setMode", mode: e.target.value });
  load();
});

for (const [id, list] of [["blockSite", "block"], ["allowSite", "allow"]]) {
  $(id).addEventListener("click", async () => {
    const result = await chrome.runtime.sendMessage({ type: "site.add", host: data.site, list }).catch(() => null);
    $("siteNote").textContent = result && result.ok ? `${result.site} ${list === "block" ? "is now blocked" : "is now always allowed"}.` : (result && result.message) || "Could not save that.";
    load();
  });
}

$("pick").addEventListener("click", async () => {
  if (!tab || !tab.id) return;
  try {
    await chrome.tabs.sendMessage(tab.id, { type: "picker.start" });
    window.close();
  } catch (_) {
    $("pick").textContent = "Reload this page first, then try again";
  }
});

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === "page.update") load();
});
setInterval(() => {
  if (data) $("time").textContent = formatTimeLeft(data.state.endsAt) || "–";
}, 1000);
load();

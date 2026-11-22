import { formatTimeLeft, hostnameOf, minutesText } from "./lib/match.js";
import { originalUrlFromHash } from "./lib/rules.js";

// blocked.html is web accessible, so a site can also put it in a frame and use its buttons. Inside a
// frame it does nothing: frames of blocked sites are not loaded at all, so this is never real.
const framed = window.top !== window;
const original = framed ? "" : originalUrlFromHash(location.hash);
const host = hostnameOf(original);
const $ = (id) => document.getElementById(id);
let info = null;
// blocked.html is web accessible, so a site can open it with any URL. Only a URL this page has
// seen blocked counts as a block, and only then may the page carry on to it when rules change.
let wasBlocked = false;

function ask(msg) {
  return chrome.runtime.sendMessage(msg).catch(() => null);
}

function render() {
  if (!info) return;
  $("site").textContent = host || "This site";
  $("task").textContent = info.task ? `You are working on ${info.task}` : "Focus mode is on";
  $("back").textContent = info.task ? `Back to ${info.task}` : "Go back";
  $("group").hidden = !info.groupName;
  $("group").textContent = info.groupName;
  $("group").style.background = info.groupColor || "#2a2f3c";
  $("time").textContent = formatTimeLeft(info.endsAt);
  const minutes = info.overrideMinutes || 5;
  $("allow").textContent = `Allow ${minutes} min`;
  $("submit").textContent = `Allow for ${minutesText(minutes)}`;
  $("allow").disabled = !info.canOverride || !wasBlocked;
  if (!wasBlocked) {
    $("form").hidden = true;
    $("note").textContent = "Kairos Shield is not blocking this site.";
  } else if (!info.canOverride) {
    $("form").hidden = true;
    $("note").textContent = info.mode === "strict" ? "Strict mode: no overrides until the session ends." : "No overrides left this session.";
  }
}

async function refresh() {
  info = await ask({ type: "page.query", url: original });
  if (!info) return;
  if (info.verdict === "blocked") {
    if (!wasBlocked && original) ask({ type: "blocked.shown", url: original });
    wasBlocked = true;
  } else if (wasBlocked) {
    // The rules changed (break started, group switched, override granted): carry on to the site.
    location.replace(original);
    return;
  }
  render();
}

function refuseFramed() {
  document.querySelector("main").replaceChildren();
  const note = document.createElement("p");
  note.textContent = "Kairos Shield's block page does not work inside another page.";
  document.querySelector("main").append(note);
}

function start() {
  $("back").addEventListener("click", () => ask({ type: "goBack" }));
  $("allow").addEventListener("click", () => {
    $("form").hidden = false;
    $("reason").focus();
  });
  $("form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const reason = $("reason").value.trim();
    if (!reason) {
      $("note").textContent = "Type a reason first.";
      return;
    }
    $("note").textContent = "Checking…";
    const result = await ask({ type: "override", target: host, reason });
    if (result && result.allowed && original && wasBlocked) location.replace(original);
    else $("note").textContent = (result && result.message) || "Not allowed.";
  });

  chrome.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "page.update") refresh();
  });
  setInterval(() => {
    if (info) $("time").textContent = formatTimeLeft(info.endsAt);
  }, 1000);

  refresh();
}

if (framed) refuseFramed();
else start();

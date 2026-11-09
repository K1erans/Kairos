// Full-size view of one saved scene, opened from the side panel's "Open in tab".
import { sceneFileName } from "./lib/importmap.js";
import { mountScene } from "./lib/sceneframe.js";
import { scenesFor } from "./lib/scenes.js";

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
let frame = null;
let scene = null;

async function show() {
  const list = await scenesFor(chrome.storage.local, params.get("url") || "");
  const entry = list.find((s) => s.id === params.get("id"));
  if (!entry) {
    $("note").textContent = "This scene is no longer saved. Open it again from the Kairos Read side panel.";
    return;
  }
  scene = entry;
  document.title = `${entry.subject} · Kairos scene`;
  $("title").textContent = entry.subject;
  frame = await mountScene($("stage"), entry);
  $("export").hidden = false;
}

$("export").addEventListener("click", () => {
  if (!scene) return;
  const url = URL.createObjectURL(new Blob([scene.html], { type: "text/html" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = sceneFileName(scene.subject);
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
});

let lastSentence = "";
chrome.runtime.onMessage.addListener((msg) => {
  const p = msg && msg.type === "player.state" ? msg.player : null;
  if (!p || !frame || !frame.contentWindow || !p.playing || !p.sentence || p.sentence === lastSentence) return;
  lastSentence = p.sentence;
  frame.contentWindow.postMessage({ type: "kairos:sentence", text: p.sentence }, "*");
});

show();

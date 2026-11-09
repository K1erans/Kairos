import { positionKey } from "./lib/sentences.js";
import { sceneFileName } from "./lib/importmap.js";
import { CLOUD_VOICE, CLOUD_MAX_RATE, CLOUD_MIN_RATE } from "./lib/player.js";
import { deleteScene as removeScene, saveScene as storeScene, scenesFor } from "./lib/scenes.js";
import { mountScene, sceneTabUrl } from "./lib/sceneframe.js";

const $ = (id) => document.getElementById(id);
const send = (msg) => chrome.runtime.sendMessage(msg).catch((err) => ({ error: String(err && err.message) }));
const cmd = (name, arg) => send({ type: "player.cmd", cmd: name, arg }).then((p) => p && !p.error && render(p));

let player = null;
let dragging = false;
let lastSentence = "";
let macVoices = [];
let voiceKey = "";
let pagePrivate = false; // the page the panel is about is on Kairos Desktop's never-analyse list
const PRIVATE_NOTE = "This site is on your never-analyse list in Kairos Desktop, so nothing from it is sent anywhere. 3D scenes are unavailable here.";

// ---- player ---------------------------------------------------------------------------------

function render(next) {
  if (!next) return;
  if (next.error) {
    // The request itself failed (the worker threw); the player state is unchanged.
    $("status").textContent = next.error;
    return;
  }
  const urlChanged = !player || positionKey(player.url) !== positionKey(next.url) || player.private !== next.private;
  player = next;
  $("title").textContent = player.loaded ? player.title : "Kairos Read";
  $("title").title = player.loaded ? player.url : "";
  $("status").textContent = player.notice || "";
  $("bridge").hidden = player.connected;
  $("bridge").textContent = `${player.connection.text} ${player.connection.hint} Reading works without it; cloud voices and 3D scenes need it.`;
  $("sentence").textContent = player.loaded ? player.sentence : "Open an article and press play, or right-click a link and choose “Read this link”.";
  $("toggle").textContent = player.playing ? "⏸" : "▶";
  $("toggle").setAttribute("aria-label", player.playing ? "Pause" : "Play");
  $("toggle").title = player.playing ? "Pause (Alt+R)" : "Play (Alt+R)";
  for (const id of ["prev", "next", "prevPara", "nextPara", "stop"]) $(id).disabled = !player.loaded;
  const progress = $("progress");
  progress.disabled = !player.loaded;
  progress.max = String(Math.max(0, player.total - 1));
  if (!dragging) progress.value = String(player.index);
  $("count").textContent = player.loaded ? `Sentence ${player.index + 1} of ${player.total}` : "";
  if (document.activeElement !== $("rate")) $("rate").value = String(player.rate);
  renderRate(player.rate);
  renderVoices();
  $("highlight").value = player.highlight;
  $("readToMe").checked = player.readToMe;
  renderQueue();
  updateSceneNote();
  renderJob();
  if (urlChanged) loadSaved();
  // Narration sync: tell the open scene which sentence is being spoken.
  if (player.playing && player.sentence && player.sentence !== lastSentence) {
    lastSentence = player.sentence;
    postToScene({ type: "kairos:sentence", text: player.sentence });
  }
}

/** A button whose accessible name says which row it acts on. */
function rowButton(text, what, onClick) {
  const button = document.createElement("button");
  button.textContent = text;
  button.title = `${text} ${what}`;
  button.setAttribute("aria-label", `${text} ${what}`);
  button.addEventListener("click", onClick);
  return button;
}

function renderQueue() {
  const list = $("queue");
  list.textContent = "";
  player.queue.forEach((entry, index) => {
    const li = document.createElement("li");
    const grow = document.createElement("span");
    grow.className = "grow";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = entry.title || entry.url;
    name.title = entry.url;
    grow.append(name);
    const what = `“${entry.title || entry.url}”`;
    li.append(
      grow,
      rowButton("Read", what, () => send({ type: "queue.play", index }).then(render)),
      rowButton("Remove", what, () => send({ type: "queue.remove", index }).then(render)),
    );
    list.append(li);
  });
  $("queueSection").hidden = player.queue.length === 0;
}

const cloudChosen = () => !!player && player.voice === CLOUD_VOICE;
const isPrivate = () => pagePrivate || (!!player && player.loaded && player.source !== "selection" && player.private);

/** The speed label; a cloud voice plays at 0.5× to 2×, so say what it will really use. */
function renderRate(rate) {
  const value = Number(rate);
  const cloud = cloudChosen() && player.cloudVoice;
  const priv = cloudChosen() && isPrivate();
  const used = cloud ? Math.min(CLOUD_MAX_RATE, Math.max(CLOUD_MIN_RATE, value)) : value;
  $("rateOut").textContent = used !== value ? `${value}× (${used}× with ${player.cloudVoice.name})` : `${value}×`;
  $("rateNote").hidden = !cloud && !priv;
  $("rateNote").textContent = priv ? "This site is on your never-analyse list, so it is read with the Mac voice." : cloud ? `${player.cloudVoice.name} plays at ${CLOUD_MIN_RATE}× to ${CLOUD_MAX_RATE}×.` : "";
}

function option(value, text, disabled = false) {
  const opt = document.createElement("option");
  opt.value = value;
  opt.textContent = text;
  opt.disabled = disabled;
  return opt;
}

/** Mac voices plus, while Kairos Desktop offers one, its cloud voice. Rebuilt only when that changes. */
function renderVoices() {
  const cloud = player ? player.cloudVoice : null;
  const key = `${macVoices.length}|${cloud ? cloud.name : ""}|${cloudChosen()}`;
  const select = $("voice");
  if (key !== voiceKey) {
    voiceKey = key;
    select.textContent = "";
    select.append(option("", "System default"));
    if (cloud) select.append(option(CLOUD_VOICE, `${cloud.name} (via Kairos Desktop)`));
    else if (cloudChosen()) select.append(option(CLOUD_VOICE, "Kairos Desktop voice (not available)", true));
    for (const v of macVoices) {
      select.append(option(v.voiceName, `${v.voiceName}${v.lang ? ` (${v.lang})` : ""}${v.remote ? " · online" : ""}`));
    }
  }
  if (player && document.activeElement !== select) select.value = player.voice;
}

function loadVoices() {
  chrome.tts.getVoices((voices) => {
    macVoices = [...(voices || [])]
      .filter((v) => v.voiceName)
      .sort((a, b) => (a.lang || "").localeCompare(b.lang || "") || a.voiceName.localeCompare(b.voiceName));
    voiceKey = "";
    renderVoices();
  });
}

$("toggle").addEventListener("click", () => cmd("toggle"));
$("stop").addEventListener("click", () => cmd("stop"));
$("prev").addEventListener("click", () => cmd("prev"));
$("next").addEventListener("click", () => cmd("next"));
$("prevPara").addEventListener("click", () => cmd("prevPara"));
$("nextPara").addEventListener("click", () => cmd("nextPara"));
$("readPage").addEventListener("click", () => cmd("start"));
$("queuePage").addEventListener("click", () => send({ type: "queue.addActive" }).then(render));
$("progress").addEventListener("input", () => { dragging = true; });
$("progress").addEventListener("change", (e) => {
  dragging = false;
  cmd("seek", Number(e.target.value));
});
$("rate").addEventListener("input", (e) => renderRate(e.target.value));
$("rate").addEventListener("change", (e) => send({ type: "settings.set", rate: Number(e.target.value) }).then(render));
$("voice").addEventListener("change", (e) => send({ type: "settings.set", voice: e.target.value }).then(render));
$("highlight").addEventListener("change", (e) => send({ type: "settings.set", highlight: e.target.value }).then(render));
$("readToMe").addEventListener("change", (e) => send({ type: "settings.set", readToMe: e.target.checked }).then(render));

// ---- 3D scenes ------------------------------------------------------------------------------

let busy = false; // a build is running (in the service worker) or candidates are being looked up
let looking = false; // candidates are being looked up
let frame = null; // the sandbox iframe
let scene = null; // {id, subject, html, url}
let sceneMessage = "";
let shownJob = ""; // the finished job this panel has already opened
let elapsedTimer = null;

/** Connection and privacy come first; otherwise the last thing the build flow had to say. */
function updateSceneNote() {
  $("sceneNote").textContent = !player || !player.connected ? "3D scenes need Kairos Desktop." : isPrivate() ? PRIVATE_NOTE : sceneMessage;
}

function setSceneMessage(text) {
  sceneMessage = text;
  updateSceneNote();
}

function setBusy(value, note) {
  looking = value;
  applyBusy();
  if (note !== undefined) setSceneMessage(note);
}

function applyBusy() {
  busy = looking || !!(player && player.sceneJob && player.sceneJob.status === "running");
  $("visualise").disabled = busy || !(player && player.connected) || isPrivate();
  for (const el of document.querySelectorAll("#candidates button, #changeForm button")) el.disabled = busy;
}

function postToScene(message) {
  if (frame && frame.contentWindow) frame.contentWindow.postMessage(message, "*");
}

async function showScene(next) {
  scene = next;
  $("sceneWrap").hidden = false;
  frame = await mountScene($("sceneFrame"), next);
}

function closeScene() {
  $("sceneFrame").textContent = "";
  $("sceneWrap").hidden = true;
  frame = null;
  scene = null;
}

async function saveScene(entry) {
  await storeScene(chrome.storage.local, entry.url, entry);
  loadSaved();
}

async function deleteScene(url, id) {
  await removeScene(chrome.storage.local, url, id);
  if (scene && scene.id === id) closeScene();
  loadSaved();
}

async function loadSaved() {
  const reply = await send({ type: "page.url" });
  const url = reply && reply.url ? reply.url : "";
  pagePrivate = !!(reply && reply.private);
  if (player) render(player);
  const saved = url ? await scenesFor(chrome.storage.local, url) : [];
  const list = $("saved");
  list.textContent = "";
  for (const entry of saved) {
    const li = document.createElement("li");
    const grow = document.createElement("span");
    grow.className = "grow";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = entry.subject;
    const why = document.createElement("span");
    why.className = "why";
    why.textContent = `Saved ${new Date(entry.at).toLocaleDateString()}`;
    grow.append(name, why);
    const what = `scene “${entry.subject}”`;
    li.append(grow, rowButton("Open", what, () => showScene({ ...entry, url })), rowButton("Delete", what, () => deleteScene(url, entry.id)));
    list.append(li);
  }
  return saved;
}

function renderCandidates(result) {
  const list = $("candidates");
  list.textContent = "";
  for (const candidate of result.candidates) {
    const li = document.createElement("li");
    const grow = document.createElement("span");
    grow.className = "grow";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = candidate.kind ? `${candidate.name} · ${candidate.kind}` : candidate.name;
    const why = document.createElement("span");
    why.className = "why";
    why.textContent = candidate.why || "";
    grow.append(name, why);
    li.append(grow, rowButton("Build", `a scene of “${candidate.name}”`, () => generate(candidate.name)));
    list.append(li);
  }
}

const clock = (ms) => {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
};

function progressText(job) {
  const what = job.kind === "edit" ? "Changing the scene" : `Building “${job.subject}”`;
  return `${what}… ${clock(Date.now() - job.startedAt)} so far. This can take a few minutes, and you can close this panel: it finishes in the background.`;
}

/** Follows the build the service worker is running: progress while it runs, the scene when it is done. */
function renderJob() {
  const job = player.sceneJob;
  const running = !!job && job.status === "running";
  $("sceneProgress").hidden = !running;
  applyBusy();
  clearInterval(elapsedTimer);
  elapsedTimer = null;
  if (running) {
    const tick = () => { $("sceneProgressText").textContent = progressText(job); };
    tick();
    elapsedTimer = setInterval(tick, 1000);
    return;
  }
  if (!job || shownJob === job.id) return;
  shownJob = job.id;
  send({ type: "scene.ack", id: job.id });
  if (job.status === "error") {
    setSceneMessage(job.error || "Could not build the scene.");
    return;
  }
  setSceneMessage("");
  openFinished(job);
}

async function openFinished(job) {
  const entry = (await scenesFor(chrome.storage.local, job.url)).find((s) => s.id === job.sceneId);
  if (!entry) return;
  await showScene({ ...entry, url: job.url });
  $("change").value = "";
  loadSaved();
}

async function generate(subject) {
  setSceneMessage("");
  const reply = await send({ type: "scene.build", subject });
  if (reply && reply.error) setSceneMessage(reply.error);
  else render(reply);
}

$("visualise").addEventListener("click", async () => {
  setBusy(true, "Looking for things worth showing in 3D…");
  const result = await send({ type: "scene.candidates" });
  if (!result || result.error) {
    setBusy(false, (result && result.error) || "Could not read this article.");
    return;
  }
  renderCandidates(result);
  setBusy(false, result.candidates.length ? "" : "Nothing in this article stood out as a 3D subject.");
});

$("cancelBuild").addEventListener("click", async () => {
  const reply = await send({ type: "scene.cancel" });
  setSceneMessage("Stopped waiting. Kairos Desktop may still finish the scene on its side.");
  if (reply) render(reply);
});

$("changeForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  const instruction = $("change").value.trim();
  if (!instruction || !scene || busy) return;
  setSceneMessage("");
  const reply = await send({ type: "scene.change", url: scene.url, id: scene.id, subject: scene.subject, instruction });
  if (reply && reply.error) setSceneMessage(reply.error);
  else render(reply);
});

$("export").addEventListener("click", () => {
  if (!scene) return;
  // The stored document is the standalone file (three.js from the CDN import map), not the rewritten one.
  const url = URL.createObjectURL(new Blob([scene.html], { type: "text/html" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = sceneFileName(scene.subject);
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
});
$("openTab").addEventListener("click", () => {
  if (scene) chrome.tabs.create({ url: sceneTabUrl(scene) });
});
$("closeScene").addEventListener("click", closeScene);

// ---- wiring ---------------------------------------------------------------------------------

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === "player.state") render(msg.player);
});
chrome.tabs.onActivated.addListener(() => loadSaved());
if (chrome.tts.onVoiceChanged) chrome.tts.onVoiceChanged.addListener(loadVoices);

loadVoices();
send({ type: "player.get" }).then(render);

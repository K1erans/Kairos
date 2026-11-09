// Kairos Read service worker: wires the player (lib/player.js) to chrome.tts, the Kairos Desktop
// bridge, the tabs being read, the read-later queue, context menus and shortcuts, and relays 3D
// scene requests to Kairos Desktop.
import { createBridge, describeConnection } from "./lib/bridge.js";
import { EMPTY_STATE, hostnameOf, matchesAny, sanitizeState, siteVerdict } from "./lib/match.js";
import { BLOCKED_MESSAGE, createPlayer } from "./lib/player.js";
import { buildPlaylist, clampRate, positionKey, rememberPosition, resumeIndex } from "./lib/sentences.js";
import { looksLikeHtmlDocument, unwrapHtml } from "./lib/importmap.js";
import { saveScene, scenesFor } from "./lib/scenes.js";

const VERSION = chrome.runtime.getManifest().version;
const CONTENT_FILES = ["vendor/Readability.js", "vendor/Readability-readerable.js", "lib/textindex.js", "lib/extract.js", "content.js"];
const DEFAULT_SETTINGS = { rate: 1, voice: "", readToMe: false, highlight: "#ffe066" };
const KEEPALIVE_MS = 20000;
const CLOUD_SENTENCE_TIMEOUT_MS = 5 * 60 * 1000;
const MAX_PAGE_BYTES = 5 * 1024 * 1024;
const NO_APP_MESSAGE = "Kairos Desktop is needed for 3D scenes. Open it and try again.";
const NEVER_ANALYSE_MESSAGE = "This site is on your never-analyse list in Kairos Desktop, so Kairos Read does not send it anywhere. It reads it with the Mac voice and cannot build a 3D scene of it.";
const NO_SETTINGS_MESSAGE = "Kairos Desktop has not sent its settings yet, so Kairos Read cannot tell whether this page may leave your Mac. Try again in a moment.";
const SCENE_DROPPED_MESSAGE =
  "The connection to Kairos Desktop dropped while the scene was being sent. Chrome cannot take a message over 1 MB from an app, so a very detailed scene can do this: ask for a simpler one.";

let appState = sanitizeState(EMPTY_STATE);
let haveState = false; // the app has sent its settings (and so its never-analyse list) on this connection
let connected = false;
let problem = null; // why the app is not reachable; see describeConnection
let settings = { ...DEFAULT_SETTINGS };
let queue = []; // [{url, title}]

// ---- speech backends --------------------------------------------------------------------------

const localSpeech = {
  speak(text, { rate, voice }, onDone) {
    let done = false;
    const once = (e) => {
      if (!done) {
        done = true;
        onDone(e);
      }
    };
    const options = {
      rate,
      enqueue: false,
      onEvent: (event) => {
        if (event.type === "end") once({ type: "end" });
        else if (event.type === "interrupted" || event.type === "cancelled") once({ type: "interrupted" });
        else if (event.type === "error") once({ type: "error", message: event.errorMessage || "unknown error" });
      },
    };
    if (voice) options.voiceName = voice;
    chrome.tts.speak(text, options, () => {
      if (chrome.runtime.lastError) once({ type: "error", message: chrome.runtime.lastError.message });
    });
  },
  stop: () => chrome.tts.stop(),
};

// Kairos Desktop fetches and plays the cloud voice itself; tts.done arrives when a sentence ends.
const cloudSpeech = {
  speak(text, { rate, url }, onDone) {
    bridge.request({ type: "tts.speak", text, rate, url }, CLOUD_SENTENCE_TIMEOUT_MS).then(
      (reply) => onDone({ type: reply.stopped ? "interrupted" : "end" }),
      (err) => onDone({ type: "error", message: err.message }),
    );
  },
  prefetch: (text, url) => bridge.send({ type: "tts.prefetch", text, url }),
  stop: () => bridge.send({ type: "tts.stop" }),
};

// ---- the player and its effects ---------------------------------------------------------------

function highlight(session, item) {
  if (session.tabId != null && item) chrome.tabs.sendMessage(session.tabId, { type: "kr.highlight", text: item.text, color: settings.highlight }).catch(() => {});
}

let keepAlive = null;
function setKeepAlive(on) {
  if (on && !keepAlive) {
    let ticks = 0;
    // An extension API call every 20 s stops Chrome suspending the worker mid-article.
    keepAlive = setInterval(() => {
      chrome.runtime.getPlatformInfo(() => void chrome.runtime.lastError);
      if (++ticks % 3 === 0) player.tick();
    }, KEEPALIVE_MS);
  } else if (!on && keepAlive) {
    clearInterval(keepAlive);
    keepAlive = null;
  }
}

// Positions are written in order, so a slow write cannot land after a newer one.
let positionChain = Promise.resolve();
function savePosition(session) {
  if (!session || session.source === "selection") return;
  const { url, index } = session;
  const total = session.items.length;
  positionChain = positionChain
    .then(async () => {
      const { positions = {} } = await chrome.storage.local.get("positions");
      await chrome.storage.local.set({ positions: rememberPosition(positions, positionKey(url), index, total) });
    })
    .catch(() => {});
}

const player = createPlayer({
  local: localSpeech,
  cloud: cloudSpeech,
  now: () => Date.now(),
  send: (msg) => bridge.send(msg),
  highlight,
  clearHighlight: (session) => {
    if (session.tabId != null) chrome.tabs.sendMessage(session.tabId, { type: "kr.clear" }).catch(() => {});
  },
  saveSession: (session) => chrome.storage.session.set({ session }).catch(() => {}),
  savePosition,
  keepAlive: setKeepAlive,
  isBlocked,
  isPrivate: (url) => offMacRefusal(url) !== null,
  nextQueued: () => queue[0] || null,
  fetchArticle: (entry) => articleFromLink(entry.url, entry.title),
  dequeue: (entry) => removeFromQueue(entry.url),
  changed: () => broadcast(),
});

const ready = Promise.all([chrome.storage.local.get(["settings", "queue"]), chrome.storage.session.get(["session", "sceneJob"])])
  .then(([local, sess]) => {
    if (sess.sceneJob) {
      // A build that was running died with the worker: its request is gone.
      sceneJob = sess.sceneJob.status === "running" ? { ...sess.sceneJob, status: "error", error: INTERRUPTED_MESSAGE } : sess.sceneJob;
    }
    settings = { ...DEFAULT_SETTINGS, ...(local.settings || {}) };
    queue = Array.isArray(local.queue) ? local.queue : [];
    player.setSettings(settings);
    if (sess.session) player.restore(sess.session); // a restarted worker comes back paused
  })
  .catch(() => {});

const bridge = createBridge({
  client: "read",
  version: VERSION,
  onMessage: (msg) => {
    ready.then(() => {
      if (msg.type === "state") {
        appState = sanitizeState(msg.state);
        haveState = true;
        player.setCloudVoice(appState.cloudVoice);
        const session = player.session();
        if (session && isBlocked(session.url)) player.stop(BLOCKED_MESSAGE);
        else player.recheckPrivacy();
      } else if (msg.type === "read.command") {
        runCommand(msg.command);
      }
    });
  },
  onProblem: (value) => {
    problem = value;
    ready.then(broadcast);
  },
  onStatus: (value) => {
    connected = value;
    if (!value) {
      appState = sanitizeState(EMPTY_STATE);
      haveState = false;
    }
    ready.then(() => {
      player.setCloudVoice(appState.cloudVoice);
      if (value) player.resendStatus();
      broadcast();
    });
  },
});
bridge.start();

chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});

/**
 * Why this page's content may not be sent off the Mac, or null when it may. Without the app's
 * settings there is no never-analyse list, so nothing counts as safe to send ("unknown").
 */
function offMacRefusal(url) {
  if (!haveState) return "unknown";
  const host = hostnameOf(url || "");
  if (!host) return "unknown";
  return matchesAny(host, appState.rules.neverAnalyse) ? "listed" : null;
}

function refusalMessage(refusal) {
  return refusal === "listed" ? NEVER_ANALYSE_MESSAGE : NO_SETTINGS_MESSAGE;
}

function snapshot() {
  const view = player.view();
  return {
    ...view,
    private: !!view.url && offMacRefusal(view.url) === "listed",
    readToMe: settings.readToMe,
    highlight: settings.highlight,
    queue,
    connected,
    connection: describeConnection(connected, problem),
    sceneJob,
    mode: appState.mode,
  };
}

function broadcast() {
  chrome.runtime.sendMessage({ type: "player.state", player: snapshot() }).catch(() => {});
}

function isBlocked(url) {
  return /^https?:\/\//i.test(url || "") && siteVerdict(url, appState) === "blocked";
}

// ---- loading articles -------------------------------------------------------------------------

async function ensureContent(tabId) {
  try {
    const pong = await chrome.tabs.sendMessage(tabId, { type: "kr.ping" });
    if (pong && pong.ok) return;
  } catch (_) {
    /* not injected yet */
  }
  await chrome.scripting.executeScript({ target: { tabId }, files: CONTENT_FILES });
}

async function loadFromTab(tab, { autoplay = true, onlyIfArticle = false } = {}) {
  if (!tab || tab.id == null || !/^https?:\/\//i.test(tab.url || "")) throw new Error("Kairos Read can only read web pages.");
  if (isBlocked(tab.url)) throw new Error(BLOCKED_MESSAGE);
  await ensureContent(tab.id);
  const reply = await chrome.tabs.sendMessage(tab.id, { type: "kr.extract", onlyIfArticle });
  if (!reply || !reply.ok) {
    if (onlyIfArticle) return false;
    throw new Error("No article found on this page.");
  }
  const items = buildPlaylist(reply.article.paragraphs);
  if (items.length === 0) throw new Error("No article found on this page.");
  const { positions = {} } = await chrome.storage.local.get("positions");
  player.load(
    {
      tabId: tab.id,
      url: tab.url,
      title: reply.article.title || tab.title || tab.url,
      source: "page",
      paragraphs: reply.article.paragraphs,
      items,
      index: resumeIndex(items, positions[positionKey(tab.url)]),
    },
    { autoplay },
  );
  return true;
}

// Only one offscreen document may exist, so parses run one at a time.
let parseChain = Promise.resolve();
function parseOffscreen(html, url) {
  const run = parseChain.then(async () => {
    const contexts = await chrome.runtime.getContexts({ contextTypes: ["OFFSCREEN_DOCUMENT"] });
    if (contexts.length === 0) {
      await chrome.offscreen.createDocument({
        url: "offscreen.html",
        reasons: ["DOM_PARSER"],
        justification: "Parse a fetched article's HTML with Readability so it can be read aloud.",
      });
    }
    try {
      return await chrome.runtime.sendMessage({ target: "offscreen", type: "parse", html, url });
    } finally {
      await chrome.offscreen.closeDocument().catch(() => {});
    }
  });
  parseChain = run.catch(() => {});
  return run;
}

const TOO_LARGE = "That page is too large to read (over 5 MB).";

/** The body as text, refusing anything over MAX_PAGE_BYTES without reading the rest. */
async function boundedText(response) {
  if (Number(response.headers.get("content-length")) > MAX_PAGE_BYTES) throw new Error(TOO_LARGE);
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > MAX_PAGE_BYTES) {
      reader.cancel().catch(() => {});
      throw new Error(TOO_LARGE);
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) {
    bytes.set(c, at);
    at += c.byteLength;
  }
  return new TextDecoder().decode(bytes);
}

/** A session for a link, fetched and parsed in the background. Throws with a readable message. */
async function articleFromLink(url, title = "") {
  if (!/^https?:\/\//i.test(url || "")) throw new Error("That link is not a web page.");
  if (isBlocked(url)) throw new Error(BLOCKED_MESSAGE);
  const response = await fetch(url, { credentials: "omit", redirect: "follow" });
  if (!response.ok) throw new Error(`The page answered ${response.status}.`);
  const type = response.headers.get("content-type") || "";
  if (type && !/html|xml/i.test(type)) throw new Error("That link is not an article.");
  const html = await boundedText(response);
  const finalUrl = response.url || url;
  if (isBlocked(finalUrl)) throw new Error(BLOCKED_MESSAGE);
  const reply = await parseOffscreen(html, finalUrl);
  if (!reply || !reply.ok) throw new Error("No article found at that link.");
  const items = buildPlaylist(reply.article.paragraphs);
  if (items.length === 0) throw new Error("No article found at that link.");
  const { positions = {} } = await chrome.storage.local.get("positions");
  return {
    tabId: null,
    url: finalUrl,
    title: reply.article.title || title || finalUrl,
    source: "link",
    paragraphs: reply.article.paragraphs,
    items,
    index: resumeIndex(items, positions[positionKey(finalUrl)]),
  };
}

async function loadSelection(text, tab) {
  const paragraphs = String(text || "").split(/\n{2,}/).map((p) => p.trim()).filter(Boolean);
  const items = buildPlaylist(paragraphs);
  if (items.length === 0) throw new Error("Nothing selected.");
  const url = (tab && tab.url) || "";
  if (isBlocked(url)) throw new Error(BLOCKED_MESSAGE);
  let tabId = null;
  if (tab && tab.id != null) {
    try {
      await ensureContent(tab.id); // only needed for highlighting; reading works without it
      tabId = tab.id;
    } catch (_) {
      tabId = null;
    }
  }
  player.load({ tabId, url, title: "Selection" + (tab && tab.title ? ` from ${tab.title}` : ""), source: "selection", paragraphs, items, index: 0 }, { autoplay: true });
}

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  return tab || null;
}

function fail(err) {
  player.notify(err && err.message ? err.message : String(err));
}

async function runCommand(command, arg) {
  await ready;
  try {
    switch (command) {
      case "toggle":
        if (player.session()) player.toggle();
        else await loadFromTab(await activeTab(), { autoplay: true });
        break;
      case "start": // read the active tab from where it was left, even if something else is loaded
        await loadFromTab(await activeTab(), { autoplay: true });
        break;
      case "stop":
        player.stop();
        break;
      case "next":
        player.next();
        break;
      case "prev":
        player.prev();
        break;
      case "nextPara":
        player.nextParagraph();
        break;
      case "prevPara":
        player.prevParagraph();
        break;
      case "seek":
        player.seek(Number(arg));
        break;
      default:
        break;
    }
  } catch (err) {
    fail(err);
  }
}

// ---- queue ------------------------------------------------------------------------------------

async function queueAdd(url, title) {
  if (!/^https?:\/\//i.test(url || "")) return;
  if (queue.some((q) => q.url === url)) return;
  queue = [...queue, { url, title: title || url }];
  await chrome.storage.local.set({ queue });
  broadcast();
}

function removeFromQueue(url) {
  queue = queue.filter((q) => q.url !== url);
  chrome.storage.local.set({ queue }).catch(() => {});
  broadcast();
}

async function queuePlay(index) {
  const entry = queue[index];
  if (!entry) return;
  const session = await articleFromLink(entry.url, entry.title);
  removeFromQueue(entry.url); // only once its article loaded
  player.load(session, { autoplay: true });
}

// ---- 3D scenes --------------------------------------------------------------------------------

async function articleForScenes() {
  const current = player.session();
  if (!current || current.source === "selection") {
    // Nothing loaded yet: load the active tab without starting to read.
    await loadFromTab(await activeTab(), { autoplay: false });
  }
  const session = player.session();
  const refusal = offMacRefusal(session.url);
  if (refusal) throw new Error(refusalMessage(refusal));
  return { title: session.title, url: session.url, text: session.paragraphs.join("\n\n") };
}

/** Scene replies are the only large messages; a dropped port while waiting for one is usually that. */
async function sceneRequest(msg, timeout) {
  try {
    return await bridge.request(msg, timeout);
  } catch (err) {
    if (err && err.code === "disconnected") throw new Error(SCENE_DROPPED_MESSAGE);
    throw err;
  }
}

async function sceneCandidates() {
  if (!connected) throw new Error(NO_APP_MESSAGE);
  const article = await articleForScenes();
  const reply = await sceneRequest({ type: "scene.candidates", title: article.title, text: article.text, url: article.url }, 120000);
  return { url: article.url, title: article.title, candidates: Array.isArray(reply.candidates) ? reply.candidates.slice(0, 3) : [] };
}

function checkedScene(reply) {
  const html = unwrapHtml(reply.html);
  if (!looksLikeHtmlDocument(html)) throw new Error("Kairos Desktop did not return a complete scene. Try again.");
  return html;
}

// A build runs here, not in the side panel, so closing the panel does not lose it. The finished
// scene is saved against its article; the panel finds the job in its snapshot when it reopens.
// Kairos Desktop has no way to abandon a build, so Cancel only stops waiting for it.
const INTERRUPTED_MESSAGE = "The scene build was interrupted when the browser stopped Kairos Read in the background. Build it again.";
let sceneJob = null; // {id, kind, subject, url, startedAt, status: "running"|"done"|"error", error, sceneId, instruction?}
let jobSeq = 0;

function setJob(next) {
  sceneJob = next;
  chrome.storage.session.set({ sceneJob }).catch(() => {});
  broadcast();
}

const jobIsRunning = () => !!sceneJob && sceneJob.status === "running";

function startJob(fields) {
  if (jobIsRunning()) throw new Error("A 3D scene is already being built. Wait for it or cancel it first.");
  if (!connected) throw new Error(NO_APP_MESSAGE);
  const job = { id: `${Date.now().toString(36)}-${(++jobSeq).toString(36)}`, url: "", sceneId: "", error: "", ...fields, startedAt: Date.now(), status: "running" };
  setJob(job);
  runJob(job).catch(() => {});
  return snapshot();
}

/** Applies a change to the job only if it is still the running one (not cancelled or replaced). */
function updateJob(job, changes) {
  if (!sceneJob || sceneJob.id !== job.id || sceneJob.status !== "running") return false;
  setJob({ ...sceneJob, ...changes });
  return true;
}

async function runJob(job) {
  try {
    let url = job.url;
    let reply;
    let sceneId = job.sceneId;
    if (job.kind === "generate") {
      const article = await articleForScenes();
      url = article.url;
      if (!updateJob(job, { url })) return;
      reply = await sceneRequest({ type: "scene.generate", title: article.title, text: article.text, subject: job.subject, url }, 600000);
      sceneId = `${Date.now().toString(36)}`;
    } else {
      const refusal = offMacRefusal(url); // the scene was made from the article at this url
      if (refusal) throw new Error(refusalMessage(refusal));
      const stored = (await scenesFor(chrome.storage.local, url)).find((s) => s.id === sceneId);
      if (!stored) throw new Error("That scene is no longer saved, so it cannot be changed.");
      reply = await sceneRequest({ type: "scene.edit", html: stored.html, instruction: job.instruction }, 600000);
    }
    const html = checkedScene(reply);
    if (!sceneJob || sceneJob.id !== job.id || sceneJob.status !== "running") return;
    await saveScene(chrome.storage.local, url, { id: sceneId, subject: job.subject, html });
    updateJob(job, { status: "done", sceneId });
  } catch (err) {
    updateJob(job, { status: "error", error: err && err.message ? err.message : String(err) });
  }
}

// ---- events -----------------------------------------------------------------------------------

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "read-selection", title: "Read selection", contexts: ["selection"] });
    chrome.contextMenus.create({ id: "read-link", title: "Read this link", contexts: ["link"] });
    chrome.contextMenus.create({ id: "queue-link", title: "Add link to Read later", contexts: ["link"] });
    chrome.contextMenus.create({ id: "read-page", title: "Read this page", contexts: ["page"] });
    chrome.contextMenus.create({ id: "queue-page", title: "Add page to Read later", contexts: ["page"] });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  // Must be asked for while the click still counts as a user gesture.
  if (tab && tab.windowId != null && info.menuItemId !== "queue-link" && info.menuItemId !== "queue-page") {
    chrome.sidePanel.open({ windowId: tab.windowId }).catch(() => {});
  }
  ready.then(async () => {
    try {
      if (info.menuItemId === "read-selection") await loadSelection(info.selectionText, tab);
      else if (info.menuItemId === "read-link") player.load(await articleFromLink(info.linkUrl), { autoplay: true });
      else if (info.menuItemId === "queue-link") await queueAdd(info.linkUrl, info.selectionText || info.linkUrl);
      else if (info.menuItemId === "read-page") await loadFromTab(tab, { autoplay: true });
      else if (info.menuItemId === "queue-page") await queueAdd(tab.url, tab.title);
    } catch (err) {
      fail(err);
    }
  });
});

chrome.commands.onCommand.addListener((command, tab) => {
  const map = { "toggle-read": "toggle", "stop-read": "stop", "next-sentence": "next", "prev-sentence": "prev" };
  if (command === "toggle-read" && tab && tab.windowId != null) chrome.sidePanel.open({ windowId: tab.windowId }).catch(() => {});
  if (map[command]) runCommand(map[command]);
});

/** tab.active is per window; the tab is only in front of the user when its window has focus too. */
async function inFront(tab) {
  if (!tab.active) return false;
  try {
    const win = await chrome.windows.get(tab.windowId);
    return !!(win && win.focused);
  } catch (_) {
    return false;
  }
}

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  ready.then(async () => {
    // The page being read navigated away: keep the audio, drop the highlighting.
    const session = player.session();
    if (session && session.tabId === tabId && changeInfo.url && positionKey(changeInfo.url) !== positionKey(session.url)) {
      session.tabId = null;
      chrome.storage.session.set({ session }).catch(() => {});
    }
    // "Read to me": start on its own when an article finishes loading in the tab in front.
    if (changeInfo.status !== "complete" || !settings.readToMe || player.isPlaying() || !/^https?:\/\//i.test(tab.url || "")) return;
    if (session && positionKey(session.url) === positionKey(tab.url)) return;
    if (isBlocked(tab.url) || !(await inFront(tab)) || player.isPlaying()) return;
    try {
      await loadFromTab(tab, { autoplay: true, onlyIfArticle: true });
    } catch (_) {
      /* not an article, or a page scripts cannot run on */
    }
  });
});

chrome.tabs.onRemoved.addListener((tabId) => {
  ready.then(() => {
    const session = player.session();
    if (session && session.tabId === tabId) {
      session.tabId = null;
      chrome.storage.session.set({ session }).catch(() => {});
    }
  });
});

async function handle(msg) {
  await ready;
  switch (msg.type) {
    case "player.get":
      return snapshot();
    case "player.cmd":
      await runCommand(msg.cmd, msg.arg);
      return snapshot();
    case "settings.set": {
      const next = { ...settings };
      if ("rate" in msg) next.rate = clampRate(msg.rate);
      if ("voice" in msg) next.voice = String(msg.voice || "");
      if ("readToMe" in msg) next.readToMe = msg.readToMe === true;
      if ("highlight" in msg && /^#[0-9a-f]{6}$/i.test(msg.highlight)) next.highlight = msg.highlight;
      const recolour = next.highlight !== settings.highlight;
      settings = next;
      // Applied before the write, so a pause that arrives meanwhile is not undone afterwards.
      player.setSettings(settings);
      const session = player.session();
      if (recolour && session) highlight(session, session.items[session.index]);
      await chrome.storage.local.set({ settings });
      return snapshot();
    }
    case "queue.add":
      await queueAdd(msg.url, msg.title);
      return snapshot();
    case "queue.addActive": {
      const tab = await activeTab();
      if (tab) await queueAdd(tab.url, tab.title);
      return snapshot();
    }
    case "queue.remove":
      if (queue[msg.index]) removeFromQueue(queue[msg.index].url);
      return snapshot();
    case "queue.play":
      await queuePlay(msg.index);
      return snapshot();
    case "scene.candidates":
      return sceneCandidates();
    case "scene.build": {
      const subject = String(msg.subject || "").trim();
      if (!subject) throw new Error("Choose something to build.");
      return startJob({ kind: "generate", subject });
    }
    case "scene.change": {
      const instruction = String(msg.instruction || "").trim();
      if (!instruction) throw new Error("Say what to change.");
      return startJob({ kind: "edit", subject: String(msg.subject || "Scene"), url: String(msg.url || ""), sceneId: String(msg.id || ""), instruction });
    }
    case "scene.cancel":
      if (jobIsRunning()) setJob(null);
      return snapshot();
    case "scene.ack":
      if (sceneJob && sceneJob.id === msg.id && sceneJob.status !== "running") setJob(null);
      return snapshot();
    case "page.url": {
      const session = player.session();
      const tab = await activeTab();
      const url = session && session.source !== "selection" ? session.url : tab ? tab.url : "";
      return { url, private: !!url && offMacRefusal(url) === "listed" };
    }
    default:
      return undefined;
  }
}

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || typeof msg.type !== "string" || msg.target === "offscreen" || msg.type === "player.state") return false;
  handle(msg).then(
    (result) => sendResponse(result),
    (err) => {
      if (!msg.type.startsWith("scene.")) fail(err); // scene errors are shown in the 3D section instead
      sendResponse({ error: err && err.message ? err.message : String(err) });
    },
  );
  return true;
});

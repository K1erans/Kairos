// The Kairos Read player: one state machine over two speech backends (chrome.tts and the cloud
// voice Kairos Desktop plays). No chrome.* use; background.js supplies the effects.
//
//   idle ──load──▶ paused ◀──pause / interrupted / error──┐
//                   │  ▲                                   │
//                 play └──────────── load ──────────── playing ──last sentence ends──▶ finishing
//                   ▼                                   ▲                                │
//                ended ◀──── nothing (more) queued ─────┼────────── next article loaded ──┘
//
// Every change of status goes through setStatus, which owns the listening clock, the keep-alive and
// the read.status report, so no route out of "playing" can leave them running.
import { clampIndex, nextParagraphIndex, prevParagraphIndex } from "./sentences.js";

export const CLOUD_VOICE = "kairos:cloud";
export const CLOUD_MIN_RATE = 0.5;
export const CLOUD_MAX_RATE = 2;

export function clampCloudRate(rate) {
  return Math.min(CLOUD_MAX_RATE, Math.max(CLOUD_MIN_RATE, Number(rate) || 1));
}

/**
 * A speech backend speaks one sentence and reports exactly one outcome through onDone:
 * {type: "end"}, {type: "interrupted"} (cut off by something else) or {type: "error", message}.
 * @typedef {{speak(text: string, opts: {rate: number, voice?: string, url?: string}, onDone: (e: object) => void): void, stop(): void, prefetch?(text: string, url?: string): void}} Speech
 *
 * @param {object} deps
 * @param {Speech} deps.local chrome.tts
 * @param {Speech} deps.cloud the cloud voice through Kairos Desktop
 * @param {() => number} deps.now
 * @param {(msg: object) => boolean} deps.send fire-and-forget to Kairos Desktop; false when not delivered
 * @param {(session: object, item: object) => void} deps.highlight
 * @param {(session: object) => void} deps.clearHighlight
 * @param {(session: object|null) => void} deps.saveSession
 * @param {(session: object) => void} deps.savePosition
 * @param {(on: boolean) => void} deps.keepAlive
 * @param {(url: string) => boolean} deps.isBlocked
 * @param {(url: string) => boolean} deps.isPrivate the page's content must not leave the Mac (never-analyse, or the list is not known)
 * @param {() => object|null} deps.nextQueued the first read-later entry, left in place
 * @param {(entry: object) => Promise<object>} deps.fetchArticle a session for a queued entry; throws on failure
 * @param {(entry: object) => void} deps.dequeue removes an entry once its article loaded
 * @param {() => void} deps.changed the view changed
 */
export function createPlayer(deps) {
  let status = "idle"; // idle | paused | playing | finishing | ended
  let session = null; // {tabId, url, title, source, paragraphs, items, index}
  let settings = { rate: 1, voice: "" };
  let cloudVoice = null; // {provider, name} while Kairos Desktop offers one
  let cloudFailed = false; // the cloud voice failed in this article; read on with the Mac voice
  let notice = "";
  let generation = 0; // bumped whenever speech is stopped, so late outcomes are ignored
  let speaking = null; // the backend of the utterance in flight
  let playSince = null;
  let lastReport = null;

  // ---- status, clock, keep-alive, read.status ------------------------------------------------

  function setStatus(next) {
    const was = status;
    if (was === "playing" && next !== "playing") flushLog(false);
    if (next === "playing" && was !== "playing") playSince = deps.now();
    status = next;
    deps.keepAlive(next === "playing" || next === "finishing");
    report();
    deps.changed();
  }

  function appState() {
    if (!session || status === "idle" || status === "finishing" || status === "ended") return "stopped";
    return status === "playing" ? "playing" : "paused";
  }

  function report() {
    const state = appState();
    const key = `${state}|${session ? session.url : ""}`;
    if (key === lastReport) return;
    const sent = deps.send({ type: "read.status", state, title: session ? session.title : "", url: session ? session.url : "" });
    lastReport = sent ? key : null;
  }

  /** Logs the seconds listened since the last flush. */
  function flushLog(stillPlaying) {
    if (playSince === null) return;
    const now = deps.now();
    const seconds = Math.round((now - playSince) / 1000);
    playSince = stillPlaying ? now : null;
    if (seconds > 0 && session) deps.send({ type: "read.log", seconds, url: session.url, title: session.title });
  }

  // ---- speech --------------------------------------------------------------------------------

  function cloudInUse() {
    return settings.voice === CLOUD_VOICE && !!cloudVoice && !cloudFailed && !(session && deps.isPrivate(session.url));
  }

  function stopSpeech() {
    generation++;
    if (speaking) speaking.stop();
    speaking = null;
  }

  function speakCurrent() {
    if (status !== "playing" || !session) return;
    const item = session.items[session.index];
    if (!item) {
      finish();
      return;
    }
    stopSpeech();
    const mine = generation;
    deps.highlight(session, item);
    deps.saveSession(session);
    deps.savePosition(session);
    deps.changed();
    if (cloudInUse()) {
      speaking = deps.cloud;
      deps.cloud.speak(item.text, { rate: clampCloudRate(settings.rate), url: session.url }, (e) => outcome(mine, true, e));
      const next = session.items[session.index + 1];
      if (next && deps.cloud.prefetch) deps.cloud.prefetch(next.text, session.url);
    } else {
      speaking = deps.local;
      const voice = settings.voice === CLOUD_VOICE ? "" : settings.voice;
      deps.local.speak(item.text, { rate: settings.rate, voice }, (e) => outcome(mine, false, e));
    }
  }

  function outcome(mine, cloud, event) {
    if (mine !== generation || status !== "playing") return;
    speaking = null;
    if (event.type === "end") {
      session.index += 1;
      speakCurrent();
    } else if (event.type === "interrupted") {
      pause();
    } else if (cloud) {
      fallBack(event.message);
    } else {
      pause(`Speech failed: ${event.message || "unknown error"}`);
    }
  }

  /** The cloud voice is gone for this article: carry on from the same sentence with the Mac voice. */
  function fallBack(reason) {
    if (cloudFailed) return;
    cloudFailed = true;
    const name = cloudVoice ? cloudVoice.name : "The cloud voice";
    notice = `${name} is unavailable${reason ? ` (${reason})` : ""}. Reading with the default Mac voice.`;
    if (status === "playing") speakCurrent();
    else deps.changed();
  }

  // ---- transitions ---------------------------------------------------------------------------

  function load(next, { autoplay }) {
    stopSpeech();
    if (session) deps.clearHighlight(session);
    // Time listened so far belongs to the article being replaced.
    if (status === "playing") flushLog(false);
    status = "paused";
    session = next;
    cloudFailed = false;
    notice = "";
    deps.saveSession(session);
    if (autoplay) play();
    else setStatus("paused");
  }

  /** A session saved before the service worker restarted. */
  function restore(saved) {
    session = saved;
    setStatus(saved ? "paused" : "idle");
  }

  function play() {
    if (!session || status === "playing" || status === "finishing") return;
    if (deps.isBlocked(session.url)) {
      stop(BLOCKED_MESSAGE);
      return;
    }
    if (status === "ended" || session.index >= session.items.length) session.index = 0;
    if (!cloudFailed) notice = "";
    setStatus("playing");
    speakCurrent();
  }

  function pause(message = "") {
    if (!session) return;
    stopSpeech();
    if (message) notice = message;
    if (status === "playing") setStatus("paused");
    else deps.changed();
    deps.saveSession(session);
  }

  /** A message for the status line that does not change playback (a link that failed to load). */
  function notify(message) {
    notice = message;
    deps.changed();
  }

  function stop(message = "") {
    stopSpeech();
    if (session) deps.clearHighlight(session);
    session = null;
    notice = message;
    deps.saveSession(null);
    setStatus("idle");
  }

  function seek(index) {
    if (!session) return;
    session.index = clampIndex(session.items, index);
    if (status === "playing") {
      speakCurrent();
      return;
    }
    stopSpeech();
    if (status === "ended") setStatus("paused");
    deps.highlight(session, session.items[session.index]);
    deps.saveSession(session);
    deps.changed();
  }

  /** The article is done: read the next queued one, removing its row only once it loaded. */
  async function finish() {
    if (!session || status === "finishing") return;
    stopSpeech();
    deps.clearHighlight(session);
    session.index = session.items.length - 1;
    deps.savePosition(session); // the last sentence clears the saved position
    setStatus("finishing");
    const finished = session;
    const entry = deps.nextQueued();
    if (entry) {
      try {
        const next = await deps.fetchArticle(entry);
        if (status !== "finishing" || session !== finished) return; // the user moved on meanwhile
        deps.dequeue(entry);
        load(next, { autoplay: true });
        return;
      } catch (err) {
        if (status !== "finishing" || session !== finished) return;
        notice = `Could not read ${entry.title || entry.url}: ${err && err.message ? err.message : err}`;
      }
    }
    deps.saveSession(session);
    setStatus("ended");
  }

  function next() {
    if (!session) return;
    if (session.index + 1 >= session.items.length) finish();
    else seek(session.index + 1);
  }

  function nextParagraph() {
    if (!session) return;
    const i = nextParagraphIndex(session.items, session.index);
    if (i >= session.items.length) finish();
    else seek(i);
  }

  function setSettings(next) {
    const respeak = status === "playing" && (next.rate !== settings.rate || next.voice !== settings.voice);
    if (next.voice !== settings.voice && next.voice === CLOUD_VOICE) cloudFailed = false;
    settings = { rate: next.rate, voice: next.voice };
    if (respeak) speakCurrent(); // the current sentence again, with the new voice or speed
    else deps.changed();
  }

  /** The never-analyse list changed: a cloud sentence in flight for a page now on it is stopped and spoken again by the Mac voice. */
  function recheckPrivacy() {
    if (status === "playing" && speaking === deps.cloud && !cloudInUse()) speakCurrent();
    else deps.changed();
  }

  function setCloudVoice(voice) {
    cloudVoice = voice || null;
    if (!cloudVoice && speaking === deps.cloud && status === "playing") fallBack("Kairos Desktop is not offering it");
    else deps.changed();
  }

  function view() {
    const item = session ? session.items[session.index] : null;
    return {
      loaded: !!session,
      playing: status === "playing",
      notice,
      title: session ? session.title : "",
      url: session ? session.url : "",
      source: session ? session.source : "",
      index: session ? session.index : 0,
      total: session ? session.items.length : 0,
      paragraph: item ? item.p : 0,
      sentence: item ? item.text : "",
      rate: settings.rate,
      voice: settings.voice,
      cloudVoice,
      cloudRate: settings.voice === CLOUD_VOICE && cloudVoice ? clampCloudRate(settings.rate) : null,
    };
  }

  return {
    load,
    restore,
    notify,
    play,
    pause,
    stop,
    seek,
    next,
    prev: () => session && seek(session.index - 1),
    nextParagraph,
    prevParagraph: () => session && seek(prevParagraphIndex(session.items, session.index)),
    toggle: () => (status === "playing" ? pause() : play()),
    finish,
    setSettings,
    setCloudVoice,
    recheckPrivacy,
    /** Called by the keep-alive timer: logs listening time in slices so a long article is not lost. */
    tick: () => status === "playing" && flushLog(true),
    /** After Kairos Desktop (re)connects it needs to hear the current state again. */
    resendStatus: () => {
      lastReport = null;
      report();
    },
    session: () => session,
    isPlaying: () => status === "playing",
    view,
  };
}

export const BLOCKED_MESSAGE = "Kairos Shield blocks this page during focus, so Kairos Read will not read it.";

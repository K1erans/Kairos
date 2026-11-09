// Kairos bridge: connection to Kairos Desktop through the native messaging host.
// Source of truth is packages/shared/bridge.js; scripts/sync-shared.sh copies it into each extension.
// See packages/shared/PROTOCOL.md.

export const HOST_NAME = "com.kairos.host";
export const OUTBOX_LIMIT = 50;

/**
 * @param {object} opts
 * @param {"shield"|"read"} opts.client
 * @param {string} opts.version
 * @param {(msg: object) => void} [opts.onMessage] pushes that are not replies to a pending request
 * @param {(connected: boolean) => void} [opts.onStatus]
 * @param {(outbox: object[]) => void} [opts.onOutbox] called when the queue of unsent messages changes,
 *        so a caller can persist it across service worker restarts
 * @param {() => {onMessage: any, onDisconnect: any, postMessage: Function, disconnect: Function}} [opts.connect]
 *        injectable for tests; defaults to chrome.runtime.connectNative
 */
export function createBridge(opts) {
  const { client, version, onMessage, onStatus, onOutbox } = opts;
  const connect = opts.connect || (() => chrome.runtime.connectNative(HOST_NAME));
  const setTimer = opts.setTimeout || ((fn, ms) => setTimeout(fn, ms));
  const pending = new Map(); // id -> {resolve, reject, timer}
  let port = null;
  let connected = false;
  let stopped = false;
  let backoff = 1000;
  let seq = 0;
  let retryTimer = null;
  let outbox = []; // messages sent with {queue: true} while the app was away

  function setConnected(value) {
    if (connected === value) return;
    connected = value;
    if (value) flushOutbox();
    if (onStatus) onStatus(value);
  }

  function setOutbox(next) {
    outbox = next;
    if (onOutbox) onOutbox(outbox.slice());
  }

  function flushOutbox() {
    let sent = 0;
    while (sent < outbox.length && post(outbox[sent])) sent++;
    if (sent) setOutbox(outbox.slice(sent));
  }

  function failPending(reason, code) {
    for (const [id, p] of pending) {
      clearTimeout(p.timer);
      const err = new Error(reason);
      err.code = code;
      p.reject(err);
      pending.delete(id);
    }
  }

  function handleDisconnect(detail) {
    port = null;
    setConnected(false);
    failPending(detail ? `Kairos Desktop is not connected (${detail})` : "Kairos Desktop is not connected", "disconnected");
    if (stopped) return;
    retryTimer = setTimer(start, backoff);
    backoff = Math.min(backoff * 2, 30000);
  }

  function handleMessage(msg) {
    if (!msg || typeof msg !== "object") return;
    // The host says "Kairos Desktop is not running" with an id-less error just before it exits;
    // anything else proves the host reached the app.
    if (msg.type === "error" && !msg.id) return;
    backoff = 1000;
    setConnected(true);
    if (msg.id && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      clearTimeout(p.timer);
      if (msg.type === "error") p.reject(new Error(msg.message || "Kairos Desktop reported an error"));
      else p.resolve(msg);
      return;
    }
    if (onMessage) onMessage(msg);
  }

  function start() {
    retryTimer = null;
    if (stopped || port) return;
    try {
      port = connect();
    } catch (err) {
      handleDisconnect();
      return;
    }
    port.onMessage.addListener(handleMessage);
    port.onDisconnect.addListener(() => {
      // Reading lastError stops Chrome logging "Unchecked runtime.lastError".
      let detail = "";
      try {
        detail = (chrome.runtime.lastError && chrome.runtime.lastError.message) || "";
      } catch (_) { /* not in Chrome (tests) */ }
      handleDisconnect(detail);
    });
    try {
      port.postMessage({ type: "hello", client, version });
    } catch (err) {
      handleDisconnect();
    }
  }

  function post(msg) {
    if (!port) return false;
    try {
      port.postMessage(msg);
      return true;
    } catch (err) {
      return false;
    }
  }

  /**
   * Fire-and-forget. Returns false when the message could not be handed to the app now.
   * With {queue: true} such a message is kept (the newest OUTBOX_LIMIT) and sent on reconnect.
   */
  function send(msg, { queue = false } = {}) {
    if (connected && post(msg)) return true;
    if (queue) setOutbox([...outbox, msg].slice(-OUTBOX_LIMIT));
    return false;
  }

  /** Sends a request with a fresh id and resolves with the reply that echoes it. */
  function request(msg, timeoutMs = 10000) {
    return new Promise((resolve, reject) => {
      if (!port) {
        reject(new Error("Kairos Desktop is not connected"));
        return;
      }
      const id = `${client}-${Date.now().toString(36)}-${(++seq).toString(36)}`;
      const timer = setTimer(() => {
        pending.delete(id);
        reject(new Error("Timed out waiting for Kairos Desktop"));
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      if (!post({ ...msg, id })) {
        pending.delete(id);
        clearTimeout(timer);
        reject(new Error("Kairos Desktop is not connected"));
      }
    });
  }

  function stop() {
    stopped = true;
    if (retryTimer) clearTimeout(retryTimer);
    if (port) {
      try { port.disconnect(); } catch (_) { /* already gone */ }
      port = null;
    }
    setConnected(false);
    failPending("Bridge stopped");
  }

  return { start, stop, send, request, isConnected: () => connected };
}

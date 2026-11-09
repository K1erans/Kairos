// Kairos Shield content script: hides page elements, runs the Jev check, shows the block overlay
// and the element picker. Classic script; rule decisions are made by the service worker.
(function () {
  "use strict";
  if (window.top !== window || window.__kairosShield) return;
  window.__kairosShield = true;

  const STYLE_ID = "kairos-shield-hide";
  const EXCERPT_WORDS = 500;
  // !important so a page stylesheet cannot hide the overlay host.
  const HOST_STYLE =
    "all:initial;display:block!important;position:fixed!important;inset:0!important;" +
    "z-index:2147483647!important;visibility:visible!important;opacity:1!important;";
  let info = null;
  let overlay = null; // {host, timer, update}
  let style = null; // the hide stylesheet, held so it can be put back
  let css = "";
  let jevToken = 0;
  let picker = null;

  function ask(msg) {
    return new Promise((resolve) => {
      try {
        chrome.runtime.sendMessage(msg, (reply) => {
          void chrome.runtime.lastError;
          resolve(reply || null);
        });
      } catch (_) {
        resolve(null); // extension was reloaded; this script is orphaned
      }
    });
  }

  // ---- element rules ------------------------------------------------------------------------

  function applySelectors(selectors) {
    css = (selectors || []).map((s) => `${s}{display:none!important}`).join("\n");
    if (!css) {
      if (style) style.remove();
      return;
    }
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      styleGuard.observe(style, { childList: true, characterData: true, subtree: true });
    }
    restore();
  }

  // Pages (or their frameworks) remove or blank nodes they did not create; put ours back.
  function restore() {
    if (css && style) {
      if (!style.isConnected) (document.head || document.documentElement).appendChild(style);
      if (style.textContent !== css) style.textContent = css;
      if (style.sheet && style.sheet.disabled) style.sheet.disabled = false;
    }
    if (overlay) {
      if (!overlay.host.isConnected) document.documentElement.appendChild(overlay.host);
      if (overlay.host.getAttribute("style") !== HOST_STYLE) overlay.host.setAttribute("style", HOST_STYLE);
      if (overlay.host.hidden) overlay.host.hidden = false;
    }
  }
  const styleGuard = new MutationObserver(restore);
  new MutationObserver(restore).observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ["style", "hidden"] });

  async function refresh(runJev) {
    info = await ask({ type: "page.query", url: location.href });
    if (!info) return;
    applySelectors(info.selectors);
    if (overlay) {
      // Disconnected, mode changed, site now listed or allowed: Jev no longer decides this page.
      if (!info.jev) removeOverlay();
      else overlay.update();
    }
    if (runJev) scheduleJev();
  }

  // ---- Jev ----------------------------------------------------------------------------------

  function pageExcerpt() {
    const text = (document.body && document.body.innerText) || "";
    return text.split(/\s+/).filter(Boolean).slice(0, EXCERPT_WORDS).join(" ");
  }

  function scheduleJev() {
    const token = ++jevToken;
    const run = async () => {
      if (token !== jevToken || overlay) return;
      if (!info || (info.mode !== "focus" && info.mode !== "strict") || info.verdict !== "unlisted") return;
      if (chrome.extension && chrome.extension.inIncognitoContext) return;
      const meta = document.querySelector('meta[name="description"], meta[property="og:description"]');
      const result = await ask({
        type: "jev.page",
        url: location.href,
        title: document.title,
        description: meta ? meta.getAttribute("content") || "" : "",
        excerpt: pageExcerpt(),
      });
      if (token !== jevToken || !result || !result.block) return;
      info = { ...info, ...result };
      showOverlay();
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => setTimeout(run, 300), { once: true });
    else setTimeout(run, 300);
  }

  // ---- block overlay ------------------------------------------------------------------------

  function timeLeft(endsAt) {
    if (typeof endsAt !== "number") return "";
    const total = Math.max(0, Math.round((endsAt - Date.now()) / 1000));
    return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
  }

  function removeOverlay() {
    if (!overlay) return;
    clearInterval(overlay.timer);
    overlay.host.remove();
    document.documentElement.style.removeProperty("overflow");
    overlay = null;
  }

  function showOverlay() {
    removeOverlay();
    document.querySelectorAll("video, audio").forEach((m) => {
      try { m.pause(); } catch (_) { /* ignore */ }
    });
    const host = document.createElement("div");
    host.setAttribute("style", HOST_STYLE);
    const root = host.attachShadow({ mode: "closed" });
    const task = info.task || "your task";
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .wrap { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center;
          background: rgba(17, 20, 28, 0.96); color: #f4f5f7;
          font: 15px/1.5 -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif; }
        .card { max-width: 440px; padding: 32px; text-align: center; }
        h1 { font-size: 22px; font-weight: 600; margin: 0 0 8px; }
        p { margin: 0 0 20px; color: #b6bac6; }
        .time { font-variant-numeric: tabular-nums; color: #f4f5f7; }
        button { font: inherit; border: 0; border-radius: 8px; padding: 10px 18px; cursor: pointer; margin: 4px; }
        .primary { background: #4f7cff; color: #fff; }
        .quiet { background: #2a2f3c; color: #dfe2ea; }
        button:disabled { opacity: 0.5; cursor: default; }
        form { display: none; margin-top: 16px; }
        input { font: inherit; width: 100%; box-sizing: border-box; padding: 10px 12px; border-radius: 8px;
          border: 1px solid #3a4050; background: #1b1f29; color: #f4f5f7; margin-bottom: 8px; }
        .note { font-size: 13px; color: #f0a35e; min-height: 18px; margin-top: 8px; }
      </style>
      <div class="wrap" role="dialog" aria-modal="true" aria-label="Kairos Shield">
        <div class="card">
          <h1>This page looks like a distraction</h1>
          <p>You are working on <strong class="task"></strong><span class="time"></span></p>
          <div>
            <button class="primary back"></button>
            <button class="quiet allow">Allow 5 min</button>
            <button class="quiet always">Always allow this site</button>
          </div>
          <form>
            <input type="text" maxlength="200" placeholder="Why do you need this page?" aria-label="Reason" />
            <button class="quiet submit" type="submit">Allow for 5 minutes</button>
          </form>
          <div class="note" aria-live="polite"></div>
        </div>
      </div>`;
    root.querySelector(".task").textContent = task;
    root.querySelector(".back").textContent = `Back to ${task}`;
    const time = root.querySelector(".time");
    const note = root.querySelector(".note");
    const allow = root.querySelector(".allow");
    const always = root.querySelector(".always");
    const submit = root.querySelector(".submit");
    const form = root.querySelector("form");
    const input = root.querySelector("input");
    const tick = () => {
      const left = timeLeft(info.endsAt);
      time.textContent = left ? ` · ${left} left` : "";
    };
    tick();
    // Rebuilt from the latest page info whenever the state changes while the overlay is up.
    const update = () => {
      const minutes = info.overrideMinutes || 5;
      allow.textContent = `Allow ${minutes} min`;
      submit.textContent = `Allow for ${minutes === 1 ? "1 minute" : `${minutes} minutes`}`;
      always.hidden = info.mode === "strict"; // an allow-for-good is not a way out of strict
      allow.disabled = !info.canOverride;
      if (info.canOverride) {
        if (/overrides/.test(note.textContent)) note.textContent = "";
        return;
      }
      form.style.display = "none";
      note.textContent = info.mode === "strict" ? "Strict mode: no overrides until the session ends." : "No overrides left this session.";
    };
    update();
    root.querySelector(".back").addEventListener("click", () => ask({ type: "goBack" }));
    always.addEventListener("click", async () => {
      note.textContent = "Saving…";
      const result = await ask({ type: "site.add", host: location.hostname, list: "allow" });
      if (result && result.ok) removeOverlay();
      else note.textContent = (result && result.message) || "Could not save that.";
    });
    allow.addEventListener("click", () => {
      form.style.display = "block";
      input.focus();
    });
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      const reason = input.value.trim();
      if (!reason) {
        note.textContent = "Type a reason first.";
        return;
      }
      note.textContent = "Checking…";
      const result = await ask({ type: "override", target: location.hostname, reason });
      if (result && result.allowed) removeOverlay();
      else note.textContent = (result && result.message) || "Not allowed.";
    });
    // Keep the page's own shortcuts from firing behind the overlay.
    for (const type of ["keydown", "keyup", "keypress"]) host.addEventListener(type, (e) => e.stopPropagation());
    document.documentElement.appendChild(host);
    document.documentElement.style.setProperty("overflow", "hidden", "important");
    overlay = { host, timer: setInterval(tick, 1000), update };
  }

  // ---- element picker -----------------------------------------------------------------------

  function stopPicker() {
    if (!picker) return;
    document.removeEventListener("mousemove", picker.move, true);
    document.removeEventListener("click", picker.click, true);
    document.removeEventListener("keydown", picker.key, true);
    picker.box.remove();
    picker.tip.remove();
    picker = null;
  }

  function toast(text) {
    const el = document.createElement("div");
    el.textContent = text;
    el.style.cssText =
      "all:initial;position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:2147483647;" +
      "background:#11141c;color:#fff;padding:10px 16px;border-radius:8px;" +
      "font:14px -apple-system,BlinkMacSystemFont,sans-serif;box-shadow:0 4px 16px rgba(0,0,0,.3)";
    document.documentElement.appendChild(el);
    setTimeout(() => el.remove(), 3000);
  }

  function startPicker() {
    stopPicker();
    const box = document.createElement("div");
    box.style.cssText =
      "all:initial;position:fixed;z-index:2147483646;pointer-events:none;border:2px solid #4f7cff;" +
      "background:rgba(79,124,255,.18);border-radius:3px;display:none";
    const tip = document.createElement("div");
    tip.textContent = "Click an element to hide it. Esc to cancel.";
    tip.style.cssText =
      "all:initial;position:fixed;left:50%;top:16px;transform:translateX(-50%);z-index:2147483647;" +
      "pointer-events:none;background:#11141c;color:#fff;padding:8px 14px;border-radius:8px;" +
      "font:13px -apple-system,BlinkMacSystemFont,sans-serif";
    document.documentElement.append(box, tip);
    let target = null;
    const move = (e) => {
      const el = document.elementFromPoint(e.clientX, e.clientY);
      if (!el || el === document.documentElement || el === document.body) return;
      target = el;
      const r = el.getBoundingClientRect();
      box.style.display = "block";
      box.style.left = r.left + "px";
      box.style.top = r.top + "px";
      box.style.width = r.width + "px";
      box.style.height = r.height + "px";
    };
    const click = async (e) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      const el = target;
      stopPicker();
      if (!el) return;
      const selector = globalThis.KairosSelector.build(el, (s) => document.querySelectorAll(s));
      if (!selector) {
        toast("Kairos Shield could not build a rule for that element.");
        return;
      }
      const result = await ask({ type: "picker.picked", host: location.hostname, selector });
      toast(result && result.ok ? "Hidden. Manage hidden elements in the Kairos Shield popup." : "Could not save that rule.");
    };
    const key = (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        stopPicker();
      }
    };
    document.addEventListener("mousemove", move, true);
    document.addEventListener("click", click, true);
    document.addEventListener("keydown", key, true);
    picker = { box, tip, move, click, key };
  }

  // ---- wiring -------------------------------------------------------------------------------

  chrome.runtime.onMessage.addListener((msg) => {
    if (!msg) return;
    if (msg.type === "page.update") refresh(!overlay);
    else if (msg.type === "page.navigated") {
      removeOverlay();
      refresh(true);
    } else if (msg.type === "picker.start") startPicker();
  });

  refresh(true);
})();

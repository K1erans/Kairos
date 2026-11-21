// Sandboxed scene host. It has an opaque origin and no extension APIs, so generated scene code
// cannot reach tabs, cookies or storage. The side panel posts one "kairos:load" message with the
// scene and the packaged three.js sources; this page turns the sources into blob: URLs, points the
// scene's import map at them and replaces itself with the scene. The blobs are created in this
// document, so they stay same-origin with the scene. The side panel makes a fresh iframe per scene.
(function () {
  "use strict";
  const ORBIT = "three/addons/controls/OrbitControls.js";
  const IMPORTMAP_RE = /<script\b[^>]*\btype\s*=\s*(["'])importmap\1[^>]*>([\s\S]*?)<\/script\s*>/i;

  // ---- no WebRTC -------------------------------------------------------------------------------
  // The CSP (default-src 'none') stops fetches and sockets but not WebRTC: a peer connection's
  // STUN/ICE traffic could carry what is being read out of the sandbox. Chrome has no CSP switch
  // for it, so the constructors are removed here before any scene code runs, as properties scene
  // code cannot redefine. A frame would bring a fresh set, so frames are locked the same way the
  // moment they appear and then taken out (frame-src 'none' leaves them nothing to show anyway).
  const WEBRTC_GLOBALS = [
    "RTCPeerConnection",
    "webkitRTCPeerConnection",
    "RTCDataChannel",
    "RTCSessionDescription",
    "RTCIceCandidate",
    "RTCIceTransport",
    "RTCDtlsTransport",
    "RTCSctpTransport",
    "RTCRtpSender",
    "RTCRtpReceiver",
    "RTCRtpTransceiver",
    "RTCCertificate",
  ];
  const FRAME_TAGS = "iframe, frame, object, embed, portal, fencedframe";

  /** Makes each WebRTC global undefined for good on `win` (a Window, or any object in tests). */
  function lockWebRtc(win) {
    if (!win) return;
    for (const name of WEBRTC_GLOBALS) {
      try {
        Object.defineProperty(win, name, { value: undefined, writable: false, enumerable: false, configurable: false });
      } catch (_) {
        // Already locked (non-configurable), or a frame of another origin, which has no way in.
      }
    }
  }

  function guardFrames(win) {
    const doc = win.document;
    const lockChildren = () => {
      let count = 0;
      try { count = win.length; } catch (_) { count = 0; }
      for (let i = 0; i < count; i++) {
        let child = null;
        try { child = win[i]; } catch (_) { child = null; }
        if (child) {
          lockWebRtc(child);
          try { guardChild(child); } catch (_) { /* another origin */ }
        }
      }
    };
    // A frame's own frames, created by script in it after it was locked.
    const guardChild = (child) => {
      if (child.__kairosLocked) return;
      Object.defineProperty(child, "__kairosLocked", { value: true });
      guardFrames(child);
    };
    const removeFrames = () => {
      try {
        for (const el of win.document.querySelectorAll(FRAME_TAGS)) el.remove();
      } catch (_) { /* no document */ }
    };
    const settle = () => {
      lockChildren();
      removeFrames();
    };

    // Everything that can put markup into the document locks any frame that came with it before
    // the caller's next line runs (a frame's window is reachable at once as window[0]).
    const wrapMethod = (proto, name) => {
      if (!proto) return;
      const desc = Object.getOwnPropertyDescriptor(proto, name);
      if (!desc || typeof desc.value !== "function") return;
      const original = desc.value;
      const wrapped = {
        [name](...args) {
          try {
            return original.apply(this, args);
          } finally {
            settle();
          }
        },
      }[name];
      try {
        Object.defineProperty(proto, name, { value: wrapped, writable: false, enumerable: desc.enumerable, configurable: false });
      } catch (_) { /* leave it */ }
    };
    const wrapSetter = (proto, name) => {
      if (!proto) return;
      const desc = Object.getOwnPropertyDescriptor(proto, name);
      if (!desc || typeof desc.set !== "function") return;
      const { get, set } = desc;
      try {
        Object.defineProperty(proto, name, {
          get,
          set(value) {
            try {
              set.call(this, value);
            } finally {
              settle();
            }
          },
          enumerable: desc.enumerable,
          configurable: false,
        });
      } catch (_) { /* leave it */ }
    };
    // The way a script reaches a frame's window: lock it on the way out.
    const wrapGetter = (proto, name) => {
      if (!proto) return;
      const desc = Object.getOwnPropertyDescriptor(proto, name);
      if (!desc || typeof desc.get !== "function") return;
      const get = desc.get;
      try {
        Object.defineProperty(proto, name, {
          get() {
            lockChildren();
            const value = get.call(this);
            try {
              const view = value && "defaultView" in value ? value.defaultView : value;
              if (view && view !== win) lockWebRtc(view);
            } catch (_) { /* another origin */ }
            return value;
          },
          set: desc.set,
          enumerable: desc.enumerable,
          configurable: false,
        });
      } catch (_) { /* leave it */ }
    };

    const proto = (name) => (win[name] && win[name].prototype) || null;
    for (const p of ["Node"]) for (const m of ["appendChild", "insertBefore", "replaceChild"]) wrapMethod(proto(p), m);
    for (const p of ["Element", "Document", "DocumentFragment", "CharacterData", "DocumentType"]) {
      for (const m of ["append", "prepend", "before", "after", "replaceWith", "replaceChildren", "moveBefore"]) wrapMethod(proto(p), m);
    }
    for (const m of ["insertAdjacentElement", "insertAdjacentHTML", "setHTMLUnsafe", "setHTML"]) wrapMethod(proto("Element"), m);
    for (const m of ["setHTMLUnsafe", "setHTML"]) wrapMethod(proto("ShadowRoot"), m);
    for (const m of ["write", "writeln", "execCommand"]) wrapMethod(proto("Document"), m);
    for (const m of ["insertNode", "surroundContents"]) wrapMethod(proto("Range"), m);
    wrapSetter(proto("Element"), "innerHTML");
    wrapSetter(proto("Element"), "outerHTML");
    wrapSetter(proto("ShadowRoot"), "innerHTML");
    for (const p of ["HTMLIFrameElement", "HTMLFrameElement", "HTMLObjectElement"]) {
      wrapGetter(proto(p), "contentWindow");
      wrapGetter(proto(p), "contentDocument");
    }
    lockChildren();
    return { settle, doc };
  }

  /** document.open() drops every listener, so these go on after it and before the scene is written. */
  function watchFrames(win, settle) {
    // An empty frame's load event fires as it is inserted, even when the parser inserts it.
    win.document.addEventListener("load", settle, true);
    if (typeof win.MutationObserver === "function") {
      new win.MutationObserver(settle).observe(win.document, { childList: true, subtree: true });
    }
  }

  // Same behaviour as lib/importmap.js (kept in step by test/importmap.test.js); duplicated
  // because a sandboxed page cannot import the extension's modules.
  function rewriteImportMap(html, urls) {
    const source = String(html || "");
    let imports = {};
    const match = IMPORTMAP_RE.exec(source);
    if (match) {
      try {
        const parsed = JSON.parse(match[2]);
        if (parsed && typeof parsed.imports === "object" && parsed.imports) imports = Object.assign({}, parsed.imports);
      } catch (_) {
        imports = {};
      }
    }
    imports.three = urls.three;
    imports[ORBIT] = urls.orbit;
    const json = JSON.stringify({ imports }).replace(/<\//g, "<\\/");
    const tag = '<script type="importmap">' + json + "</" + "script>";
    if (match) return source.slice(0, match.index) + tag + source.slice(match.index + match[0].length);
    const head = /<head\b[^>]*>/i.exec(source);
    if (head) return source.slice(0, head.index + head[0].length) + tag + source.slice(head.index + head[0].length);
    const htmlTag = /<html\b[^>]*>/i.exec(source);
    if (htmlTag) return source.slice(0, htmlTag.index + htmlTag[0].length) + tag + source.slice(htmlTag.index + htmlTag[0].length);
    const doctype = /^\s*<!doctype[^>]*>/i.exec(source);
    if (doctype) return doctype[0] + tag + source.slice(doctype[0].length);
    return tag + source;
  }

  function onMessage(event) {
    const data = event.data;
    if (!data || data.type !== "kairos:load" || typeof data.html !== "string") return;
    window.removeEventListener("message", onMessage);
    let html = data.html;
    if (typeof data.three === "string" && typeof data.orbit === "string") {
      const three = URL.createObjectURL(new Blob([data.three], { type: "text/javascript" }));
      const orbit = URL.createObjectURL(new Blob([data.orbit], { type: "text/javascript" }));
      html = rewriteImportMap(html, { three, orbit });
    }
    document.open();
    if (guard) watchFrames(window, guard.settle);
    document.write(html);
    document.close();
    if (guard) guard.settle();
  }

  let guard = null;
  if (typeof window !== "undefined") {
    lockWebRtc(window);
    try {
      guard = guardFrames(window);
    } catch (_) {
      guard = null;
    }
    window.addEventListener("message", onMessage);
    if (window.parent && window.parent !== window) window.parent.postMessage({ type: "kairos:ready" }, "*");
  }
  if (typeof globalThis !== "undefined") globalThis.KairosSandbox = { rewriteImportMap, lockWebRtc, WEBRTC_GLOBALS };
})();

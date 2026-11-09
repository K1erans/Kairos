// Sandboxed scene host. It has an opaque origin and no extension APIs, so generated scene code
// cannot reach tabs, cookies or storage. The side panel posts one "kairos:load" message with the
// scene and the packaged three.js sources; this page turns the sources into blob: URLs, points the
// scene's import map at them and replaces itself with the scene. The blobs are created in this
// document, so they stay same-origin with the scene. The side panel makes a fresh iframe per scene.
(function () {
  "use strict";
  const ORBIT = "three/addons/controls/OrbitControls.js";
  const IMPORTMAP_RE = /<script\b[^>]*\btype\s*=\s*(["'])importmap\1[^>]*>([\s\S]*?)<\/script\s*>/i;

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
    document.write(html);
    document.close();
  }

  if (typeof window !== "undefined") {
    window.addEventListener("message", onMessage);
    if (window.parent && window.parent !== window) window.parent.postMessage({ type: "kairos:ready" }, "*");
  }
  if (typeof globalThis !== "undefined") globalThis.KairosSandbox = { rewriteImportMap };
})();

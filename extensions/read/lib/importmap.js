// Rewrites a scene's import map so three.js loads from the extension's packaged copy.
// See "Scene contract" in packages/shared/PROTOCOL.md. No chrome.* or DOM use.

export const ORBIT_SPECIFIER = "three/addons/controls/OrbitControls.js";
const IMPORTMAP_RE = /<script\b[^>]*\btype\s*=\s*(["'])importmap\1[^>]*>([\s\S]*?)<\/script\s*>/i;

/**
 * @param {string} html scene document
 * @param {{three: string, orbit: string}} urls replacement URLs (blob: URLs in the sandbox page)
 * @returns {string} html with "three" and the OrbitControls specifier pointed at urls;
 *          other entries (the unpkg "three/addons/" prefix) are kept as written; the sandbox CSP refuses the network, so only the two blob: entries resolve
 */
export function rewriteImportMap(html, urls) {
  const source = String(html || "");
  let imports = {};
  const match = IMPORTMAP_RE.exec(source);
  if (match) {
    try {
      const parsed = JSON.parse(match[2]);
      if (parsed && typeof parsed.imports === "object" && parsed.imports) imports = { ...parsed.imports };
    } catch (_) {
      imports = {};
    }
  }
  imports.three = urls.three;
  imports[ORBIT_SPECIFIER] = urls.orbit;
  // "</" must not appear inside the script element.
  const json = JSON.stringify({ imports }).replace(/<\//g, "<\\/");
  const tag = `<script type="importmap">${json}</script>`;
  if (match) return source.slice(0, match.index) + tag + source.slice(match.index + match[0].length);
  // No import map: one must come before the first module script.
  const head = /<head\b[^>]*>/i.exec(source);
  if (head) return source.slice(0, head.index + head[0].length) + tag + source.slice(head.index + head[0].length);
  const htmlTag = /<html\b[^>]*>/i.exec(source);
  if (htmlTag) return source.slice(0, htmlTag.index + htmlTag[0].length) + tag + source.slice(htmlTag.index + htmlTag[0].length);
  const doctype = /^\s*<!doctype[^>]*>/i.exec(source);
  if (doctype) return doctype[0] + tag + source.slice(doctype[0].length);
  return tag + source;
}

/** Cheap check that a reply is a whole HTML document and not, say, a markdown fence. */
export function looksLikeHtmlDocument(html) {
  const s = String(html || "").trim().toLowerCase();
  return (s.startsWith("<!doctype html") || s.startsWith("<html")) && s.includes("</html>");
}

/** Strips a ```html fence if the model wrapped the document in one. */
export function unwrapHtml(text) {
  const s = String(text || "").trim();
  const m = /^```(?:html)?\s*\n([\s\S]*?)\n```$/i.exec(s);
  return m ? m[1].trim() : s;
}

export function sceneFileName(subject) {
  const slug = String(subject || "scene").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 50);
  return `${slug || "scene"}.html`;
}

// Kairos Read content script (injected on demand): extracts the article and highlights the
// sentence being spoken with the CSS Custom Highlight API, which leaves the page's DOM untouched.
(function () {
  "use strict";
  if (window.__kairosRead) return;
  window.__kairosRead = true;

  const HIGHLIGHT = "kairos-read";
  const STYLE_ID = "kairos-read-highlight";
  const SKIP = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEXTAREA", "TEMPLATE", "IFRAME", "SVG", "CANVAS"]);
  const supported = typeof CSS !== "undefined" && CSS.highlights && typeof Highlight === "function";
  const TI = globalThis.KairosTextIndex;
  let nodes = [];
  let index = null;
  let cursor = 0;
  let articleRoot = null; // the live element holding the article, found after extraction
  let scope = null; // what the current index covers

  function textNodes(root) {
    const out = [];
    if (!root) return out;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode(node) {
        const parent = node.parentElement;
        if (!parent || SKIP.has(parent.tagName.toUpperCase())) return NodeFilter.FILTER_REJECT;
        return /\S/.test(node.nodeValue) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
      },
    });
    for (let n = walker.nextNode(); n; n = walker.nextNode()) out.push(n);
    return out;
  }

  function buildIndex(root) {
    scope = root;
    nodes = textNodes(root);
    index = TI.buildIndex(nodes.map((n) => n.nodeValue));
  }

  // Readability works on a copy, so the live article is found again by its text: the closest
  // element holding both its first and last substantial paragraph. Navigation, sidebars and
  // related links outside it are then never highlighted.
  function findArticleRoot(paragraphs) {
    const body = document.body;
    const long = paragraphs.filter((p) => p.length >= 40);
    const pick = long.length ? long : paragraphs;
    if (!body || pick.length === 0) return body;
    const all = textNodes(body);
    const idx = TI.buildIndex(all.map((n) => n.nodeValue));
    const last = TI.locate(idx, pick[pick.length - 1], 0);
    if (!last) return body;
    // The copy of the first paragraph nearest before the last one: a teaser or a link in the
    // navigation that repeats it earlier on the page is passed over.
    let first = null;
    for (let from = 0; ; ) {
      const hit = TI.locate(idx, pick[0], from);
      if (!hit || hit.end <= from || hit.end > last.end) break;
      first = hit;
      from = hit.end;
    }
    if (!first) return body;
    const end = all[last.endNode];
    let el = all[first.startNode].parentElement;
    while (el && !el.contains(end)) el = el.parentElement;
    return el || body;
  }

  function setColor(color) {
    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      (document.head || document.documentElement).appendChild(style);
    }
    const safe = /^#[0-9a-f]{6}$/i.test(color || "") ? color : "#ffe066";
    const css = `::highlight(${HIGHLIGHT}){background-color:${safe};color:#111}`;
    if (style.textContent !== css) style.textContent = css;
  }

  function rangeIn(text) {
    const hit = TI.locate(index, text, cursor);
    if (!hit) return null;
    const start = nodes[hit.startNode];
    const end = nodes[hit.endNode];
    if (!start || !end || !start.isConnected || !end.isConnected) return null;
    if (hit.startOffset > start.nodeValue.length || hit.endOffset > end.nodeValue.length) return null;
    const range = document.createRange();
    range.setStart(start, hit.startOffset);
    range.setEnd(end, hit.endOffset);
    // The page may have changed its text since the index was built.
    if (TI.strip(range.toString()) !== TI.strip(text)) return null;
    cursor = hit.end;
    return range;
  }

  function rangeFor(text) {
    const root = articleRoot && articleRoot.isConnected ? articleRoot : document.body;
    if (!index || scope !== root) buildIndex(root);
    let range = rangeIn(text);
    if (range) return range;
    // A miss or stale offsets: rebuild from the live page and search again from the top.
    buildIndex(root);
    cursor = 0;
    range = rangeIn(text);
    if (range || root === document.body) return range;
    // A selection read from outside the article.
    buildIndex(document.body);
    return rangeIn(text);
  }

  function clear() {
    if (supported) CSS.highlights.delete(HIGHLIGHT);
  }

  function highlight(text, color) {
    if (!supported) return;
    const range = rangeFor(text);
    if (!range) {
      clear();
      return;
    }
    setColor(color);
    CSS.highlights.set(HIGHLIGHT, new Highlight(range));
    const rect = range.getBoundingClientRect();
    const margin = window.innerHeight * 0.2;
    if (rect.height === 0 || rect.top < margin || rect.bottom > window.innerHeight - margin) {
      const el = range.startContainer.parentElement;
      if (el) el.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }

  function extract(onlyIfArticle) {
    if (onlyIfArticle && !globalThis.KairosExtract.probablyArticle(document)) return { ok: false };
    // Readability rewrites the document it is given, so it gets a copy.
    const article = globalThis.KairosExtract.extract(document.cloneNode(true));
    if (!article) return { ok: false };
    articleRoot = findArticleRoot(article.paragraphs);
    index = null;
    cursor = 0;
    return { ok: true, article, url: location.href };
  }

  chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
    if (!msg || typeof msg.type !== "string" || !msg.type.startsWith("kr.")) return;
    if (msg.type === "kr.ping") sendResponse({ ok: true });
    else if (msg.type === "kr.extract") sendResponse(extract(msg.onlyIfArticle === true));
    else if (msg.type === "kr.highlight") highlight(msg.text, msg.color);
    else if (msg.type === "kr.clear") {
      clear();
      cursor = 0;
    }
  });
})();

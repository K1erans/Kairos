// Article extraction with Mozilla Readability. Classic script used by the content script and the
// offscreen document; needs the Readability global (vendor/Readability.js) and a DOM.
(function (root) {
  "use strict";

  const BLOCKS = "p, h1, h2, h3, h4, h5, h6, li, blockquote, pre, figcaption, dd, dt, td, th";

  function clean(text) {
    return String(text || "").replace(/\s+/g, " ").trim();
  }

  // Leaf blocks in document order; when most of the text sits outside them, fall back to lines.
  function paragraphsFromContent(contentHtml, textContent) {
    const doc = new DOMParser().parseFromString(contentHtml || "", "text/html");
    const out = [];
    for (const el of doc.body.querySelectorAll(BLOCKS)) {
      if (el.querySelector(BLOCKS)) continue;
      const text = clean(el.textContent);
      if (text) out.push(text);
    }
    const whole = clean(textContent);
    const covered = out.reduce((n, p) => n + p.length, 0);
    if (whole && covered < whole.length * 0.6) {
      return String(textContent).split(/\n+/).map(clean).filter(Boolean);
    }
    return out;
  }

  /**
   * @param {Document} doc a document Readability may modify (pass a clone of the live page)
   * @returns {null | {title: string, byline: string, lang: string, paragraphs: string[]}}
   */
  function extract(doc) {
    let article = null;
    try {
      article = new root.Readability(doc).parse();
    } catch (_) {
      article = null;
    }
    if (!article || !clean(article.textContent)) return null;
    const paragraphs = paragraphsFromContent(article.content, article.textContent);
    if (paragraphs.length === 0) return null;
    return {
      title: clean(article.title),
      byline: clean(article.byline),
      lang: article.lang || "",
      paragraphs,
    };
  }

  function probablyArticle(doc) {
    try {
      return typeof root.isProbablyReaderable === "function" ? root.isProbablyReaderable(doc) : true;
    } catch (_) {
      return false;
    }
  }

  root.KairosExtract = { extract, probablyArticle, clean };
})(typeof globalThis !== "undefined" ? globalThis : this);

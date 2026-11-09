// Offscreen document: parses fetched HTML (service workers have no DOMParser) and runs Readability.
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || msg.target !== "offscreen" || msg.type !== "parse") return;
  try {
    const doc = new DOMParser().parseFromString(String(msg.html || ""), "text/html");
    // Readability resolves relative links against the document's base.
    const base = doc.createElement("base");
    base.href = msg.url;
    doc.head.prepend(base);
    const article = globalThis.KairosExtract.extract(doc);
    sendResponse(article ? { ok: true, article } : { ok: false });
  } catch (err) {
    sendResponse({ ok: false, error: String(err && err.message) });
  }
});

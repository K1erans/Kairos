// Finds a spoken sentence in the page's text nodes. Classic script (content scripts cannot be
// modules): attaches to globalThis for the content script and the node tests.
// Matching ignores whitespace, so line breaks and inline-element boundaries do not matter, but a
// match must start and end on word boundaries, so "the cat" is not found inside "the category".
(function (root) {
  "use strict";

  const WS = /\s/;
  // Letters and digits of scripts that separate words with spaces. Han, kana, Thai and similar
  // scripts do not, so they get no word-boundary checks.
  const UNSPACED = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Thai}\p{Script=Lao}\p{Script=Khmer}\p{Script=Myanmar}]/u;
  function isWordChar(c) {
    return !!c && /[\p{L}\p{N}]/u.test(c) && !UNSPACED.test(c);
  }

  function strip(text) {
    return String(text).replace(/\s+/g, "");
  }

  /**
   * @param {string[]} texts the text of each text node, in document order
   * @returns {{text: string, starts: number[], texts: string[], gaps: Uint8Array}} starts[i] = offset
   *          of node i in text; gaps[k] = 1 when whitespace or a node edge comes before character k
   */
  function buildIndex(texts) {
    const starts = new Array(texts.length);
    const parts = new Array(texts.length);
    let total = 0;
    for (let i = 0; i < texts.length; i++) {
      starts[i] = total;
      parts[i] = strip(texts[i]);
      total += parts[i].length;
    }
    const gaps = new Uint8Array(total + 1);
    gaps[total] = 1;
    for (let i = 0; i < texts.length; i++) {
      let k = starts[i];
      let gap = true; // a node edge counts as a break: blocks are not always separated by whitespace
      for (const ch of String(texts[i])) {
        if (WS.test(ch)) {
          gap = true;
          continue;
        }
        for (let u = 0; u < ch.length; u++) gaps[k++] = gap && u === 0 ? 1 : 0;
        gap = false;
      }
    }
    return { text: parts.join(""), starts, texts, gaps };
  }

  // Positions in the stripped sentence that follow whitespace between two words.
  function wordBreaks(sentence) {
    const breaks = [];
    const raw = String(sentence).trim();
    let k = 0;
    let gap = false;
    let prev = "";
    for (const ch of raw) {
      if (WS.test(ch)) {
        gap = true;
        continue;
      }
      if (gap && isWordChar(prev) && isWordChar(ch)) breaks.push(k);
      k += ch.length;
      prev = ch;
      gap = false;
    }
    return breaks;
  }

  // Last node whose start is <= pos and that actually holds characters at pos.
  function nodeAt(index, pos) {
    let lo = 0;
    let hi = index.starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (index.starts[mid] <= pos) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  // Offset in the raw node text of the n-th (0-based) non-whitespace character.
  function rawOffset(raw, n) {
    let seen = 0;
    for (let i = 0; i < raw.length; i++) {
      if (WS.test(raw[i])) continue;
      if (seen === n) return i;
      seen++;
    }
    return raw.length;
  }

  function fits(index, needle, breaks, at) {
    const { text, gaps } = index;
    const end = at + needle.length;
    if (isWordChar(needle[0]) && at > 0 && !gaps[at] && isWordChar(text[at - 1])) return false;
    if (isWordChar(needle[needle.length - 1]) && end < text.length && !gaps[end] && isWordChar(text[end])) return false;
    return breaks.every((k) => gaps[at + k]);
  }

  function find(index, needle, breaks, from) {
    let at = index.text.indexOf(needle, from);
    while (at >= 0 && !fits(index, needle, breaks, at)) at = index.text.indexOf(needle, at + 1);
    return at;
  }

  /**
   * @param {number} from position in the stripped text to search from (falls back to the start)
   * @returns {null | {startNode: number, startOffset: number, endNode: number, endOffset: number, end: number}}
   *          node numbers index the texts array; offsets are into the raw node text; end is the
   *          stripped position just past the match (pass it as `from` next time)
   */
  function locate(index, sentence, from) {
    const needle = strip(sentence);
    if (!needle || !index || !index.text) return null;
    const breaks = wordBreaks(sentence);
    let at = find(index, needle, breaks, Math.max(0, from || 0));
    if (at < 0 && from > 0) at = find(index, needle, breaks, 0);
    if (at < 0) return null;
    const last = at + needle.length - 1;
    const startNode = nodeAt(index, at);
    const endNode = nodeAt(index, last);
    return {
      startNode,
      startOffset: rawOffset(index.texts[startNode], at - index.starts[startNode]),
      endNode,
      endOffset: rawOffset(index.texts[endNode], last - index.starts[endNode]) + 1,
      end: at + needle.length,
    };
  }

  root.KairosTextIndex = { buildIndex, locate, strip };
})(typeof globalThis !== "undefined" ? globalThis : this);

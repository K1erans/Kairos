// Sentence splitting and playlist navigation for Kairos Read. No chrome.* use.

const ABBREVIATIONS = new Set([
  "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "inc", "ltd", "co", "corp", "no", "fig", "vol",
  "gen", "col", "lt", "sgt", "capt", "rev", "hon", "gov", "sen", "rep", "pres", "approx", "est", "dept", "univ",
  "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "e.g", "i.e", "cf", "al", "ca",
  "a.m", "p.m", "u.s", "u.k", "ph.d",
]);

// Abbreviations that also end sentences: "No. 5" and "Fig. 3" continue, "No. She refused." does not.
const ENDS_BEFORE_CAPITAL = new Set(["no", "inc", "al", "fig"]);

const MAX_SENTENCE = 600;

/** textBefore ends just before the full stop; next is the first character of the following word. */
function isAbbreviation(textBefore, next) {
  const m = /([A-Za-z][A-Za-z.]*)$/.exec(textBefore);
  if (!m) return false;
  const word = m[1];
  if (word.length === 1 && /[A-Z]/.test(word)) return true; // initial, as in "J. K. Rowling"
  const lower = word.toLowerCase();
  if (ENDS_BEFORE_CAPITAL.has(lower) && /\p{Lu}/u.test(next || "")) return false;
  return ABBREVIATIONS.has(lower);
}

/** Breaks an over-long sentence at clause marks so no utterance runs for minutes. */
function chunkLong(sentence) {
  if (sentence.length <= MAX_SENTENCE) return [sentence];
  const out = [];
  let rest = sentence;
  while (rest.length > MAX_SENTENCE) {
    const window = rest.slice(0, MAX_SENTENCE);
    let cut = Math.max(...["; ", ", ", ": ", " – ", " — ", "，", "、", "；"].map((mark) => window.lastIndexOf(mark)));
    if (cut < MAX_SENTENCE / 3) cut = window.lastIndexOf(" ");
    if (cut <= 0) cut = MAX_SENTENCE - 1;
    // Never split a surrogate pair (emoji, rarer CJK): cut before it instead.
    if (/[\uD800-\uDBFF]/.test(rest[cut])) cut -= 1;
    out.push(rest.slice(0, cut + 1).trim());
    rest = rest.slice(cut + 1).trim();
  }
  if (rest) out.push(rest);
  return out;
}

/** Splits a paragraph into sentences. Whitespace is collapsed. */
export function splitSentences(paragraph) {
  const text = String(paragraph || "").replace(/\s+/g, " ").trim();
  if (!text) return [];
  const out = [];
  let start = 0;
  // Either a terminator run, optional closing quotes/brackets, whitespace, then something that opens
  // a sentence (a capital, a digit, or a letter from a script without case), or a CJK full-width
  // terminator, which needs no space after it.
  const re = /([.!?…؟]+)(["'”’)\]»]*)\s+(?=["'“‘(\[«]*[\p{Lu}\p{Lt}\p{N}\p{Lo}])|([。！？]+)([」』”’）)]*)\s*(?=\S)/gu;
  let m;
  while ((m = re.exec(text))) {
    const dotAt = m.index;
    const mark = m[1] || m[3];
    const close = m[1] ? m[2] : m[4];
    if (mark === "." && isAbbreviation(text.slice(start, dotAt), text.slice(re.lastIndex).replace(/^["'“‘(\[«]+/, "")[0])) continue;
    const end = dotAt + mark.length + close.length;
    const sentence = text.slice(start, end).trim();
    if (sentence) out.push(sentence);
    start = re.lastIndex;
  }
  const tail = text.slice(start).trim();
  if (tail) out.push(tail);
  return out.flatMap(chunkLong);
}

/** Flat list of {text, p} where p is the paragraph number. */
export function buildPlaylist(paragraphs) {
  const items = [];
  let p = 0;
  for (const para of paragraphs || []) {
    const sentences = splitSentences(para);
    if (sentences.length === 0) continue;
    for (const text of sentences) items.push({ text, p });
    p++;
  }
  return items;
}

export function clampIndex(items, index) {
  if (!items.length) return 0;
  return Math.min(Math.max(0, index | 0), items.length - 1);
}

/** First sentence of the next paragraph, or items.length when there is none (= finished). */
export function nextParagraphIndex(items, index) {
  const current = items[index];
  if (!current) return items.length;
  for (let i = index + 1; i < items.length; i++) if (items[i].p !== current.p) return i;
  return items.length;
}

/**
 * Start of the current paragraph, or of the previous one when already at its start
 * (the way "previous track" works on a music player).
 */
export function prevParagraphIndex(items, index) {
  const i = clampIndex(items, index);
  if (!items.length) return 0;
  const startOf = (p) => items.findIndex((it) => it.p === p);
  const start = startOf(items[i].p);
  if (i > start) return start;
  return items[i].p > 0 ? startOf(items[i].p - 1) : 0;
}

export const MIN_RATE = 0.5;
export const MAX_RATE = 3;
export function clampRate(rate) {
  const n = Number(rate);
  if (!Number.isFinite(n)) return 1;
  return Math.round(Math.min(MAX_RATE, Math.max(MIN_RATE, n)) * 100) / 100;
}

/** Where to resume: the saved sentence, unless it is missing, out of range or the last one. */
export function resumeIndex(items, saved) {
  if (!Number.isInteger(saved) || saved <= 0 || saved >= items.length - 1) return 0;
  return saved;
}

export const MAX_POSITIONS = 500;

/**
 * Saved positions with this article's updated: its key moves to the end (newest), a finished
 * article's is dropped, and the oldest entries go past the limit.
 */
export function rememberPosition(positions, key, index, total, limit = MAX_POSITIONS) {
  const out = { ...positions };
  delete out[key];
  if (index < total - 1) out[key] = index;
  const keys = Object.keys(out);
  for (const old of keys.slice(0, Math.max(0, keys.length - limit))) delete out[old];
  return out;
}

/** Drops the fragment so "#section" links share one saved position. */
export function positionKey(url) {
  const s = String(url || "");
  const i = s.indexOf("#");
  return i >= 0 ? s.slice(0, i) : s;
}

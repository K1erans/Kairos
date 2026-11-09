import test from "node:test";
import assert from "node:assert/strict";
import {
  buildPlaylist,
  clampIndex,
  clampRate,
  nextParagraphIndex,
  positionKey,
  prevParagraphIndex,
  rememberPosition,
  resumeIndex,
  splitSentences,
} from "../lib/sentences.js";

test("splits on sentence ends and collapses whitespace", () => {
  assert.deepEqual(splitSentences("Hello  world.\nThis is   Kairos! Does it work? Yes."), ["Hello world.", "This is Kairos!", "Does it work?", "Yes."]);
});

test("does not split on abbreviations, initials or decimals", () => {
  assert.deepEqual(splitSentences("Dr. Smith met J. K. Rowling at 3.5 p.m. in the U.S. Capitol. They talked."), [
    "Dr. Smith met J. K. Rowling at 3.5 p.m. in the U.S. Capitol.",
    "They talked.",
  ]);
  assert.deepEqual(splitSentences("It costs $4.20 e.g. Tuesday prices. Fine."), ["It costs $4.20 e.g. Tuesday prices.", "Fine."]);
});

test("keeps closing quotes with their sentence and handles opening quotes", () => {
  assert.deepEqual(splitSentences('She said "Stop." Then he left. “Why?” she asked.'), ['She said "Stop."', "Then he left.", "“Why?”", "she asked."].slice(0, 2).concat(["“Why?” she asked."]));
});

test("ellipsis and no trailing punctuation", () => {
  assert.deepEqual(splitSentences("Wait… What happened"), ["Wait…", "What happened"]);
  assert.deepEqual(splitSentences("   "), []);
  assert.deepEqual(splitSentences(null), []);
});

test("very long sentences are chunked at clause marks without losing text", () => {
  const clause = "the quick brown fox jumps over the lazy dog near the river bank";
  const long = Array.from({ length: 30 }, () => clause).join(", ") + ".";
  const parts = splitSentences(long);
  assert.ok(parts.length > 1);
  for (const p of parts) assert.ok(p.length <= 600, String(p.length));
  assert.equal(parts.join(" "), long);
});

test("playlist numbers paragraphs and skips empty ones", () => {
  const items = buildPlaylist(["One. Two.", "  ", "Three."]);
  assert.deepEqual(items, [{ text: "One.", p: 0 }, { text: "Two.", p: 0 }, { text: "Three.", p: 1 }]);
  assert.deepEqual(buildPlaylist(null), []);
});

test("paragraph navigation", () => {
  const items = buildPlaylist(["Aa. Bb. Cc.", "Dd. Ee.", "Ff."]);
  assert.equal(nextParagraphIndex(items, 0), 3);
  assert.equal(nextParagraphIndex(items, 4), 5);
  assert.equal(nextParagraphIndex(items, 5), items.length, "past the last paragraph means finished");
  assert.equal(prevParagraphIndex(items, 2), 0, "mid-paragraph goes to its start");
  assert.equal(prevParagraphIndex(items, 3), 0, "at a start goes to the previous paragraph");
  assert.equal(prevParagraphIndex(items, 5), 3);
  assert.equal(prevParagraphIndex(items, 0), 0);
  assert.equal(prevParagraphIndex([], 0), 0);
});

test("a single capital before a full stop is read as an initial, not a sentence end", () => {
  assert.deepEqual(splitSentences("Ask John F. Kennedy. He knew."), ["Ask John F. Kennedy.", "He knew."]);
});

test("clamps", () => {
  const items = buildPlaylist(["Aa. Bb. Cc."]);
  assert.equal(clampIndex(items, -4), 0);
  assert.equal(clampIndex(items, 99), 2);
  assert.equal(clampIndex([], 3), 0);
  assert.equal(clampRate(0.1), 0.5);
  assert.equal(clampRate(9), 3);
  assert.equal(clampRate("1.25"), 1.25);
  assert.equal(clampRate("x"), 1);
});

test("resume ignores missing, out-of-range and final positions", () => {
  const items = buildPlaylist(["Aa. Bb. Cc. Dd."]);
  assert.equal(resumeIndex(items, 2), 2);
  assert.equal(resumeIndex(items, 3), 0, "finished articles start over");
  assert.equal(resumeIndex(items, 40), 0);
  assert.equal(resumeIndex(items, undefined), 0);
  assert.equal(resumeIndex(items, -1), 0);
});

test("positions are keyed without the fragment", () => {
  assert.equal(positionKey("https://a.com/post?x=1#part-2"), "https://a.com/post?x=1");
  assert.equal(positionKey("https://a.com/post"), "https://a.com/post");
});

test("no, inc, al and fig end a sentence when a capitalised word follows", () => {
  assert.deepEqual(splitSentences("No. She refused."), ["No.", "She refused."]);
  assert.deepEqual(splitSentences("Acme Inc. The firm grew."), ["Acme Inc.", "The firm grew."]);
  assert.deepEqual(splitSentences("See fig. 3 and No. 5 for details."), ["See fig. 3 and No. 5 for details."]);
});

test("Cyrillic, CJK and Arabic text splits into sentences", () => {
  assert.deepEqual(splitSentences("Привет. Как дела? Хорошо!"), ["Привет.", "Как дела?", "Хорошо!"]);
  assert.deepEqual(splitSentences("今日は晴れです。明日は雨でしょう！本当？"), ["今日は晴れです。", "明日は雨でしょう！", "本当？"]);
  assert.deepEqual(splitSentences("كيف حالك؟ أنا بخير."), ["كيف حالك؟", "أنا بخير."]);
});

test("chunking an unbroken run never cuts a surrogate pair", () => {
  const long = "a" + "😀".repeat(400);
  const parts = splitSentences(long);
  assert.ok(parts.length > 1);
  for (const p of parts) assert.ok(!/^[\uDC00-\uDFFF]|[\uD800-\uDBFF]$/.test(p), "no half pairs at chunk edges");
  assert.equal(parts.join(""), long);
});

test("remembering a position keeps it as the newest entry and drops the oldest past the limit", () => {
  const positions = { a: 1, b: 2, c: 3 };
  assert.deepEqual(Object.entries(rememberPosition(positions, "a", 4, 10, 3)), [["b", 2], ["c", 3], ["a", 4]]);
  assert.deepEqual(Object.keys(rememberPosition(positions, "d", 1, 10, 3)), ["b", "c", "d"]);
  assert.deepEqual(rememberPosition(positions, "b", 9, 10, 3), { a: 1, c: 3 }, "the last sentence clears it");
});

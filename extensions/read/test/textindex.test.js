import test from "node:test";
import assert from "node:assert/strict";
import "../lib/textindex.js";

const { buildIndex, locate } = globalThis.KairosTextIndex;

// Reads the located range back out of the node texts, the way a DOM Range would.
function textOf(texts, hit) {
  if (hit.startNode === hit.endNode) return texts[hit.startNode].slice(hit.startOffset, hit.endOffset);
  let out = texts[hit.startNode].slice(hit.startOffset);
  for (let i = hit.startNode + 1; i < hit.endNode; i++) out += texts[i];
  return out + texts[hit.endNode].slice(0, hit.endOffset);
}
const squash = (s) => s.replace(/\s+/g, "");

test("finds a sentence inside one text node", () => {
  const texts = ["  Menu  ", "\n   The cat sat.  The dog ran.\n"];
  const index = buildIndex(texts);
  const hit = locate(index, "The dog ran.", 0);
  assert.equal(hit.startNode, 1);
  assert.equal(textOf(texts, hit), "The dog ran.");
});

test("finds a sentence that spans inline elements and line breaks", () => {
  const texts = ["Intro. The ", "quick\n  brown", " fox", " jumps. Next one."];
  const index = buildIndex(texts);
  const hit = locate(index, "The quick brown fox jumps.", 0);
  assert.equal(hit.startNode, 0);
  assert.equal(hit.endNode, 3);
  assert.equal(squash(textOf(texts, hit)), "Thequickbrownfoxjumps.");
  assert.equal(texts[hit.startNode].slice(hit.startOffset, hit.startOffset + 3), "The");
  assert.equal(texts[hit.endNode].slice(0, hit.endOffset), " jumps.");
});

test("searches forward from the last match so repeats are found in order", () => {
  const texts = ["Yes. No. ", "Yes. Maybe."];
  const index = buildIndex(texts);
  const first = locate(index, "Yes.", 0);
  const second = locate(index, "Yes.", first.end);
  assert.equal(first.startNode, 0);
  assert.equal(second.startNode, 1);
  assert.equal(textOf(texts, second), "Yes.");
});

test("falls back to the start when the cursor is past the match (user skipped back)", () => {
  const texts = ["Alpha. Beta. Gamma."];
  const index = buildIndex(texts);
  const gamma = locate(index, "Gamma.", 0);
  const alpha = locate(index, "Alpha.", gamma.end);
  assert.equal(textOf(texts, alpha), "Alpha.");
});

test("a match ending exactly at a node boundary stays in that node", () => {
  const texts = ["One two.", "Three."];
  const hit = locate(buildIndex(texts), "One two.", 0);
  assert.equal(hit.endNode, 0);
  assert.equal(hit.endOffset, 8);
});

test("returns null when the text is not on the page", () => {
  const index = buildIndex(["Something else entirely."]);
  assert.equal(locate(index, "Not here.", 0), null);
  assert.equal(locate(index, "   ", 0), null);
  assert.equal(locate(buildIndex([]), "x", 0), null);
});

test("matches whole words: 'the cat' is not found inside 'the category'", () => {
  const texts = ["Browse the category list. ", "Then the cat sat."];
  const hit = locate(buildIndex(texts), "the cat", 0);
  assert.ok(hit, "found");
  assert.equal(hit.startNode, 1);
  assert.equal(textOf(texts, hit), "the cat");
});

test("words split by whitespace in the sentence must be split on the page too", () => {
  assert.equal(locate(buildIndex(["Notacat here."]), "Not a cat", 0), null);
  assert.ok(locate(buildIndex(["Read ", "this", " now."]), "Read this now.", 0), "inline element edges still match");
});

test("text without spaces between words (CJK) still matches mid-run", () => {
  const texts = ["今日は晴れです。明日は雨でしょう！"];
  const hit = locate(buildIndex(texts), "明日は雨でしょう！", 0);
  assert.equal(textOf(texts, hit), "明日は雨でしょう！");
});

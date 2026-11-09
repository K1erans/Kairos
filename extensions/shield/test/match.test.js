import test from "node:test";
import assert from "node:assert/strict";
import { formatTimeLeft, hostMatches, normalizeDomain, sanitizeState, siteVerdict } from "../lib/match.js";

const state = (over = {}, rules = {}) => sanitizeState({ mode: "focus", rules: { blockSites: ["youtube.com"], ...rules }, ...over });

test("normalizeDomain strips scheme, path, port, www and wildcards", () => {
  assert.equal(normalizeDomain("https://www.YouTube.com/watch?v=1"), "youtube.com");
  assert.equal(normalizeDomain("*.reddit.com"), "reddit.com");
  assert.equal(normalizeDomain("localhost:3000"), "localhost");
  assert.equal(normalizeDomain("not a domain"), "");
  assert.equal(normalizeDomain(null), "");
});

test("hostMatches follows the protocol: equal, or ends with '.' + entry", () => {
  assert.ok(hostMatches("youtube.com", "youtube.com"));
  assert.ok(hostMatches("m.youtube.com", "youtube.com"));
  assert.ok(!hostMatches("notyoutube.com", "youtube.com"));
  assert.ok(!hostMatches("youtube.com.evil.net", "youtube.com"));
  assert.ok(!hostMatches("youtube.com", ""));
});

test("siteVerdict: modes without site rules are off", () => {
  assert.equal(siteVerdict("https://youtube.com/", state({ mode: "off" })), "off");
  assert.equal(siteVerdict("https://youtube.com/", state({ mode: "break" })), "off");
  assert.equal(siteVerdict("https://youtube.com/", state({ mode: "strict" })), "blocked");
});

test("siteVerdict: allow list and live temporary allows beat the block list", () => {
  const now = 1000;
  assert.equal(siteVerdict("https://www.youtube.com/x", state(), now), "blocked");
  assert.equal(siteVerdict("https://music.youtube.com/", state({}, { allowSites: ["music.youtube.com"] }), now), "allowed");
  assert.equal(siteVerdict("https://youtube.com/", state({}, { temporaryAllows: { "youtube.com": 2000 } }), now), "allowed");
  assert.equal(siteVerdict("https://youtube.com/", state({}, { temporaryAllows: { "youtube.com": 500 } }), now), "blocked");
  assert.equal(siteVerdict("https://example.org/", state(), now), "unlisted");
});

test("sanitizeState fills every field from a partial push", () => {
  const s = sanitizeState({ mode: "bogus", rules: { blockSites: "nope" } });
  assert.equal(s.mode, "off");
  assert.deepEqual(s.rules.blockSites, []);
  assert.deepEqual(s.rules.hiddenElements, {});
  assert.equal(s.rules.jevThreshold, 0.7);
  assert.equal(s.currentTask, null);
});

test("formatTimeLeft", () => {
  assert.equal(formatTimeLeft(61000, 0), "1:01");
  assert.equal(formatTimeLeft(0, 5000), "0:00");
  assert.equal(formatTimeLeft(null), "");
});

test("Unicode domain entries match their punycode hostnames", () => {
  assert.equal(normalizeDomain("münchen.de"), "xn--mnchen-3ya.de");
  assert.ok(hostMatches("www.xn--mnchen-3ya.de", "münchen.de"));
  assert.equal(siteVerdict("https://münchen.de/", state({}, { blockSites: ["münchen.de"] })), "blocked");
});

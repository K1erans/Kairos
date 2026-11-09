import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (name) => readFileSync(new URL(`../${name}`, import.meta.url), "utf8");
const html = read("sidepanel.html");
const css = read("sidepanel.css");

function buttons() {
  return [...html.matchAll(/<button\b([^>]*)>([^<]*)<\/button>/g)].map(([, attrs, text]) => ({
    id: /id="([^"]+)"/.exec(attrs)?.[1],
    title: /title="([^"]*)"/.exec(attrs)?.[1],
    aria: /aria-label="([^"]*)"/.exec(attrs)?.[1],
    text: text.trim(),
  }));
}

test("every control in the player has a title and an aria-label", () => {
  const controls = buttons().filter((b) => ["prevPara", "prev", "toggle", "next", "nextPara", "stop"].includes(b.id));
  assert.equal(controls.length, 6);
  for (const b of controls) {
    assert.ok(b.title, `${b.id} has a title`);
    assert.ok(b.aria, `${b.id} has an aria-label`);
  }
});

test("no two player controls share a glyph, and Play is not the Next sentence glyph", () => {
  const glyphs = buttons().filter((b) => ["prevPara", "prev", "toggle", "next", "nextPara", "stop"].includes(b.id)).map((b) => b.text);
  assert.equal(new Set(glyphs).size, glyphs.length, glyphs.join(" "));
});

test("the other inputs are labelled", () => {
  for (const id of ["progress", "change"]) assert.match(html, new RegExp(`id="${id}"[^>]*aria-label="`), id);
  assert.match(html, /<label class="field">\s*<span>Speed/);
});

// ---- contrast -----------------------------------------------------------------------------------

const luminance = (hex) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};
const CANVAS = { light: "#ffffff", dark: "#121212" }; // Chrome's Canvas for color-scheme light / dark

/** The light and dark colours a rule gives `color`: a literal, or a custom property set on :root and again in the dark media query. */
function colours(selector) {
  const rule = new RegExp(`(?:^|\\n)${selector.replace(/[.]/g, "\\.")}\\s*\\{[^}]*?\\bcolor:\\s*([^;}]+)`).exec(css);
  assert.ok(rule, `${selector} sets a colour`);
  const value = rule[1].trim();
  const prop = /^var\((--[\w-]+)\)$/.exec(value);
  if (!prop) return { light: value, dark: value };
  const dark = /@media \(prefers-color-scheme: dark\)\s*\{([\s\S]*?)\n\}/.exec(css);
  const light = new RegExp(`${prop[1]}:\\s*(#[0-9a-f]{6})`, "i").exec(css.slice(0, dark ? dark.index : undefined));
  const darker = dark && new RegExp(`${prop[1]}:\\s*(#[0-9a-f]{6})`, "i").exec(dark[1]);
  assert.ok(light && darker, `${prop[1]} has a light and a dark value`);
  return { light: light[1], dark: darker[1] };
}

test("status and secondary text reach 4.5:1 in light and dark", () => {
  for (const selector of [".status", "h2", ".meta", "li .why"]) {
    const c = colours(selector);
    for (const scheme of ["light", "dark"]) assert.ok(ratio(c[scheme], CANVAS[scheme]) >= 4.5, `${selector} ${scheme} ${ratio(c[scheme], CANVAS[scheme]).toFixed(2)}`);
  }
});

test("motion respects prefers-reduced-motion", () => {
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(read("content.js"), /matchMedia\("\(prefers-reduced-motion: reduce\)"\)/);
});

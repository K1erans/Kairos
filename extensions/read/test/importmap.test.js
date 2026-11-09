import test from "node:test";
import assert from "node:assert/strict";
import { ORBIT_SPECIFIER, looksLikeHtmlDocument, rewriteImportMap, sceneFileName, unwrapHtml } from "../lib/importmap.js";
import "../sandbox.js";

const URLS = { three: "blob:null/three", orbit: "blob:null/orbit" };
const CONTRACT = `<!doctype html><html><head><meta charset="utf-8">
<script type="importmap">
{"imports":{"three":"https://unpkg.com/three@0.170.0/build/three.module.js",
"three/addons/":"https://unpkg.com/three@0.170.0/examples/jsm/"}}
</script></head><body><script type="module">import * as THREE from "three";</script></body></html>`;

function importsOf(html) {
  const m = /<script type="importmap">([\s\S]*?)<\/script>/.exec(html);
  return JSON.parse(m[1]).imports;
}

test("points three and OrbitControls at the packaged copies, keeps the addons prefix as fallback", () => {
  const out = rewriteImportMap(CONTRACT, URLS);
  assert.deepEqual(importsOf(out), {
    three: URLS.three,
    "three/addons/": "https://unpkg.com/three@0.170.0/examples/jsm/",
    [ORBIT_SPECIFIER]: URLS.orbit,
  });
  assert.equal(out.match(/importmap/g).length, 1);
  assert.ok(out.includes('import * as THREE from "three"'), "the rest of the scene is untouched");
  assert.ok(out.indexOf("importmap") < out.indexOf('type="module"'), "import map stays before the module");
});

test("tolerates single quotes and extra attributes on the import map tag", () => {
  const html = `<html><head><script async type='importmap' id="m">{"imports":{"three":"x"}}</script></head></html>`;
  assert.equal(importsOf(rewriteImportMap(html, URLS)).three, URLS.three);
});

test("adds an import map when the scene has none or it is broken", () => {
  for (const html of ["<!doctype html><html><head><title>t</title></head><body></body></html>", "<html><body></body></html>", "<!DOCTYPE html><p>hi", "<p>bare</p>"]) {
    const out = rewriteImportMap(html, URLS);
    assert.equal(importsOf(out).three, URLS.three, html);
    assert.equal(importsOf(out)[ORBIT_SPECIFIER], URLS.orbit);
  }
  const broken = `<html><head><script type="importmap">{not json</script></head></html>`;
  assert.deepEqual(importsOf(rewriteImportMap(broken, URLS)), { three: URLS.three, [ORBIT_SPECIFIER]: URLS.orbit });
});

test("a URL containing </ cannot close the script element", () => {
  const out = rewriteImportMap(CONTRACT, { three: "blob:x</script><b>", orbit: "o" });
  assert.equal(out.match(/<\/script>/g).length, 2);
});

test("the sandbox page's copy behaves the same as the module", () => {
  const sandbox = globalThis.KairosSandbox.rewriteImportMap;
  for (const html of [CONTRACT, "<html><body></body></html>", "<p>bare</p>", `<html><head><script type="importmap">{bad</script></head></html>`]) {
    assert.equal(sandbox(html, URLS), rewriteImportMap(html, URLS));
  }
});

test("scene replies are unwrapped and checked", () => {
  assert.equal(unwrapHtml("```html\n<!doctype html><html></html>\n```"), "<!doctype html><html></html>");
  assert.equal(unwrapHtml("  <html></html> "), "<html></html>");
  assert.ok(looksLikeHtmlDocument("<!DOCTYPE html><html><body></body></html>"));
  assert.ok(!looksLikeHtmlDocument("Sorry, I cannot build that."));
  assert.ok(!looksLikeHtmlDocument("<!doctype html><html><body>cut off"));
});

test("export file names", () => {
  assert.equal(sceneFileName("Saturn V rocket!"), "saturn-v-rocket.html");
  assert.equal(sceneFileName(""), "scene.html");
  assert.equal(sceneFileName("???"), "scene.html");
});

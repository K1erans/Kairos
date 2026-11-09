import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const manifest = JSON.parse(readFileSync(new URL("../manifest.json", import.meta.url), "utf8"));
const directives = (csp) => Object.fromEntries(csp.split(";").map((d) => d.trim().split(/\s+/)).filter((d) => d[0]).map(([name, ...values]) => [name, values]));

test("scene code in the sandbox page has no way onto the network", () => {
  const csp = directives(manifest.content_security_policy.sandbox);
  assert.deepEqual(csp["default-src"], ["'none'"]);
  assert.equal("connect-src" in csp, false, "connect-src stays at default-src 'none'");
  for (const [name, values] of Object.entries(csp)) {
    for (const v of values) assert.doesNotMatch(v, /^(https?|wss?):|\*/, `${name} allows ${v}`);
  }
  assert.ok(csp["script-src"].includes("blob:"), "three.js is injected as blob: URLs");
  assert.deepEqual(csp["frame-src"], ["'none'"]);
  assert.deepEqual(csp["form-action"], ["'none'"]);
});

test("the side panel cannot be made to navigate the scene frame off the extension", () => {
  assert.ok(directives(manifest.content_security_policy.extension_pages)["frame-src"].every((v) => v === "'self'"));
});

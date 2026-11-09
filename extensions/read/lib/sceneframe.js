// Puts a scene into a sandboxed iframe (sandbox.html) and hands it the packaged three.js sources.
// Used by the side panel and by the full-size scene tab. Needs chrome.runtime and the DOM.
let vendor = null; // the packaged three.js sources, fetched once

async function loadVendor() {
  if (!vendor) {
    const [three, orbit] = await Promise.all(
      ["vendor/three.module.js", "vendor/OrbitControls.js"].map((path) => fetch(chrome.runtime.getURL(path)).then((r) => r.text())),
    );
    vendor = { three, orbit };
  }
  return vendor;
}

/** Replaces the contents of `holder` with a fresh iframe showing the scene; returns the iframe. */
export async function mountScene(holder, scene) {
  const sources = await loadVendor();
  holder.textContent = "";
  // A new iframe per scene: the sandbox page replaces its own document when it loads one.
  const iframe = document.createElement("iframe");
  iframe.title = `3D scene: ${scene.subject}`;
  iframe.src = chrome.runtime.getURL("sandbox.html");
  const onReady = (event) => {
    if (event.source !== iframe.contentWindow || !event.data || event.data.type !== "kairos:ready") return;
    window.removeEventListener("message", onReady);
    iframe.contentWindow.postMessage({ type: "kairos:load", html: scene.html, three: sources.three, orbit: sources.orbit }, "*");
  };
  window.addEventListener("message", onReady);
  holder.append(iframe);
  return iframe;
}

export function sceneTabUrl(scene) {
  return `${chrome.runtime.getURL("scene.html")}?${new URLSearchParams({ url: scene.url, id: scene.id })}`;
}

// Builds a CSS selector for a clicked element. Classic script (content scripts cannot be modules):
// it attaches to globalThis so the content script and the node tests can both use it.
(function (root) {
  "use strict";

  // CSS.escape, with the CSSOM algorithm as a fallback for node tests.
  function cssEscape(value) {
    if (root.CSS && typeof root.CSS.escape === "function") return root.CSS.escape(value);
    const s = String(value);
    let out = "";
    for (let i = 0; i < s.length; i++) {
      const ch = s[i];
      const c = s.charCodeAt(i);
      if (c === 0) out += "\uFFFD";
      else if ((c >= 1 && c <= 0x1f) || c === 0x7f || (/[0-9]/.test(ch) && (i === 0 || (i === 1 && s[0] === "-")))) out += `\\${c.toString(16)} `;
      else if (i === 0 && ch === "-" && s.length === 1) out += "\\-";
      else if (c >= 0x80 || /[-_a-zA-Z0-9]/.test(ch)) out += ch;
      else out += "\\" + ch;
    }
    return out;
  }

  // Generated names (hashes, long digit runs, CSS-in-JS prefixes) change between page loads.
  function isStableName(name) {
    if (!name || name.length > 40) return false;
    if (/\d{3,}/.test(name)) return false;
    if (/^(css|sc|jsx|svelte|emotion|chakra)-[a-z0-9]{4,}$/i.test(name)) return false;
    if (/^[a-z0-9_-]*[A-Z0-9]{2,}[a-z0-9_-]*[A-Z0-9]{2,}/.test(name) && /\d/.test(name)) return false;
    if (/^[a-f0-9]{6,}$/i.test(name)) return false;
    if (/(^|[-_])(active|hover|focus|selected|open|visible|hidden|is-[a-z]+)$/i.test(name)) return false;
    return /^-?[_a-zA-Z][\w-]*$/.test(name);
  }

  function tagOf(el) {
    return String(el.tagName || "").toLowerCase();
  }

  function nthOfType(el) {
    const parent = el.parentElement;
    if (!parent) return 0;
    const same = Array.from(parent.children).filter((c) => c.tagName === el.tagName);
    return same.length > 1 ? same.indexOf(el) + 1 : 0;
  }

  function segmentFor(el) {
    const tag = tagOf(el);
    const testId = el.getAttribute && el.getAttribute("data-testid");
    if (testId && testId.length < 60) return { text: `${tag}[data-testid="${cssEscape(testId)}"]`, strong: true };
    const classes = Array.from(el.classList || []).filter(isStableName).slice(0, 2);
    let text = tag + classes.map((c) => "." + cssEscape(c)).join("");
    const nth = nthOfType(el);
    if (nth && classes.length === 0) text += `:nth-of-type(${nth})`;
    return { text, strong: false, nth, hasClasses: classes.length > 0 };
  }

  // The hide stylesheet lives in the document and cannot reach into shadow trees, so an element
  // inside one is hidden through its outermost shadow host.
  function lightDomElement(el) {
    let node = el;
    for (let r = node.getRootNode && node.getRootNode(); r && r.host; r = node.getRootNode && node.getRootNode()) node = r.host;
    return node;
  }

  /**
   * A selector that matches exactly this element in the document, or "" when there is none.
   * @param {Element} picked
   * @param {(selector: string) => ArrayLike<Element>} query the document's matches for a selector
   */
  function build(picked, query) {
    if (!picked || !picked.tagName) return "";
    const el = lightDomElement(picked);
    const onlyEl = (selector) => {
      try {
        const found = query(selector);
        return found.length === 1 && found[0] === el;
      } catch (_) {
        return false; // not a valid selector
      }
    };
    if (el.id && isStableName(el.id)) {
      const byId = "#" + cssEscape(el.id);
      if (onlyEl(byId)) return byId;
    }
    const parts = [];
    for (let node = el; node && node.tagName; node = node.parentElement) {
      const tag = tagOf(node);
      if (tag === "html" || tag === "body") break;
      if (node !== el && node.id && isStableName(node.id)) {
        parts.unshift("#" + cssEscape(node.id));
        const sel = parts.join(" > ");
        return onlyEl(sel) ? sel : "";
      }
      const seg = segmentFor(node);
      parts.unshift(seg.text);
      let sel = parts.join(" > ");
      if (onlyEl(sel)) return sel;
      // Same classes on siblings: pin the position before climbing further.
      if (seg.hasClasses && seg.nth) {
        parts[0] = `${seg.text}:nth-of-type(${seg.nth})`;
        sel = parts.join(" > ");
        if (onlyEl(sel)) return sel;
      }
    }
    return "";
  }

  root.KairosSelector = { build, isStableName, cssEscape };
})(typeof globalThis !== "undefined" ? globalThis : this);

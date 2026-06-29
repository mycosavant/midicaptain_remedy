// Tiny DOM helpers — keeps the UI modules free of framework weight.

/**
 * Create an element.
 * @param {string} tag
 * @param {object} [props] attributes; `class`, `dataset`, `style`, `html`, and
 *   `onClick`-style listeners are special-cased.
 * @param {...(Node|string|null|false|Array)} children
 */
export function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === "class") node.className = v;
    else if (k === "dataset") Object.assign(node.dataset, v);
    else if (k === "style") node.style.cssText = v;
    else if (k === "html") node.innerHTML = v;
    else if (k.startsWith("on") && typeof v === "function")
      node.addEventListener(k.slice(2).toLowerCase(), v);
    else node.setAttribute(k, v === true ? "" : v);
  }
  appendChildren(node, children);
  return node;
}

export function appendChildren(node, children) {
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    node.append(c.nodeType ? c : document.createTextNode(String(c)));
  }
  return node;
}

export function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
  return node;
}

const hex2 = (n) => (n & 0xff).toString(16).padStart(2, "0");

/** [r,g,b] -> "#rrggbb" for <input type=color>. */
export function colorToHex(color) {
  const [r, g, b] = color || [0, 0, 0];
  return `#${hex2(r)}${hex2(g)}${hex2(b)}`;
}

/** "#rrggbb" -> [r,g,b]. */
export function hexToRgb(h) {
  const s = (h || "#000000").replace("#", "");
  return [0, 2, 4].map((i) => parseInt(s.slice(i, i + 2), 16) || 0);
}

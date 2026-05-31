// Placeholder for the "OEM Files" tab. The OEM (Paint Audio) preset-file editor
// is the original single-page app (webapp/index.html + app.js). It edits a
// DIFFERENT model (SuperMode page*.txt / GeekMode gekey*.dat for the stock
// firmware) than the Rust firmware's live RuntimeConfig, so the two are kept as
// separate modes rather than fused. This tab just links across.

import { el, clear } from "./dom.js";

export function createOemPlaceholder(root) {
  clear(root);
  root.append(
    el(
      "div",
      { class: "placeholder" },
      el("h2", {}, "OEM preset files"),
      el(
        "p",
        {},
        "The OEM preset-file editor (SuperMode ",
        el("code", {}, "page*.txt"),
        " / GeekMode ",
        el("code", {}, "gekey*.dat"),
        ") for the stock Paint Audio firmware is the original single-page app.",
      ),
      el("p", {}, el("a", { class: "btn primary", href: "index.html" }, "Open the OEM file editor →")),
      el(
        "p",
        { class: "hint" },
        "That format is a file you copy onto the device's USB drive. The ",
        el("strong", {}, "Live Device"),
        " tab here is different: it talks to the Rust/Embassy firmware over USB-CDC and edits its live RuntimeConfig directly — connect, read, edit, write, and the change is live without a reboot.",
      ),
    ),
  );
}

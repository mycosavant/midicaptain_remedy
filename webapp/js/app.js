// Entry point for the Live Device sync editor (live.html). Sets up the two
// tabs and boots their controllers.

import { createLiveEditor } from "./ui/live.js";
import { createOemPlaceholder } from "./ui/oem.js";

function boot() {
  const liveRoot = document.getElementById("tab-live");
  const oemRoot = document.getElementById("tab-oem");
  createLiveEditor(liveRoot);
  createOemPlaceholder(oemRoot);

  const buttons = Array.from(document.querySelectorAll("nav.tabs button"));
  function activate(name) {
    for (const b of buttons) b.classList.toggle("active", b.dataset.tab === name);
    liveRoot.hidden = name !== "live";
    oemRoot.hidden = name !== "oem";
  }
  for (const b of buttons) b.addEventListener("click", () => activate(b.dataset.tab));
  activate("live");
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", boot);
} else {
  boot();
}

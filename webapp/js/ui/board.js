// The 10-footswitch board view for the live editor. Renders each slot with its
// LED colour, label, and a short action summary; clicking selects it.

import { el, clear, colorToHex } from "./dom.js";
import { SLOT_NAMES, describeAction } from "../device/runtimeconfig.js";

/**
 * @param {HTMLElement} container
 * @param {{name:string, buttons:Array}} page
 * @param {number|null} selectedIndex
 * @param {(index:number)=>void} onSelect
 */
export function renderBoard(container, page, selectedIndex, onSelect) {
  clear(container);
  for (let i = 0; i < SLOT_NAMES.length; i++) {
    const b = page.buttons[i] || {};
    const longBound = b.on_long_press && b.on_long_press.kind !== "none";
    container.append(
      el(
        "button",
        {
          class: "switch" + (i === selectedIndex ? " selected" : ""),
          type: "button",
          onClick: () => onSelect(i),
          title: `${SLOT_NAMES[i]} — click to edit`,
        },
        el("span", { class: "switch-slot" }, SLOT_NAMES[i]),
        el("span", { class: "switch-led", style: `background:${colorToHex(b.color)}` }),
        el("span", { class: "switch-label" }, b.label || "—"),
        el("span", { class: "switch-act" }, describeAction(b.on_press)),
        longBound
          ? el("span", { class: "switch-act switch-act-long" }, "long: " + describeAction(b.on_long_press))
          : null,
      ),
    );
  }
}

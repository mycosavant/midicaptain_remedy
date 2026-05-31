// Per-switch inspector: edits label, LED colour, and the press / long-press
// actions of the selected button. Edits flow back through `onEdit(mutator, opts)`
// so the controller (live.js) owns the config and decides what to re-render.

import { el, clear, colorToHex, hexToRgb } from "./dom.js";
import {
  ACTION_KINDS,
  SYSEX_CMDS,
  SLOT_NAMES,
  LABEL_CAP,
  MIDI_MAX,
  byteLen,
} from "../device/runtimeconfig.js";

function field(name, ...controls) {
  return el("label", { class: "field" }, el("span", { class: "field-name" }, name), ...controls);
}

function numInput(value, min, max, onVal) {
  return el("input", {
    type: "number",
    min,
    max,
    value,
    class: "num",
    oninput: (e) => {
      const n = parseInt(e.target.value, 10);
      onVal(Number.isNaN(n) ? min : n);
    },
  });
}

function selectInput(options, current, onVal) {
  const s = el("select", { class: "sel", onchange: (e) => onVal(e.target.value) });
  for (const o of options) {
    const opt = el("option", { value: o.value }, o.label);
    if (String(o.value) === String(current)) opt.selected = true;
    s.append(opt);
  }
  return s;
}

function defaultAction(kind) {
  switch (kind) {
    case "midi_cc":
      return { kind, cc: 0, value: 0 };
    case "program_change":
      return { kind, program: 0 };
    case "sysex":
      return { kind, sysex: "amp_type", param: 0 };
    case "page_change":
      return { kind, page: 0 };
    default:
      return { kind };
  }
}

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

function actionEditor(title, slot, action, ctx, onEdit) {
  const fs = el("fieldset", { class: "action" }, el("legend", {}, title));
  fs.append(
    field(
      "Type",
      selectInput(
        ACTION_KINDS.map((a) => ({ value: a.kind, label: a.label })),
        action.kind,
        (kind) => onEdit((b) => (b[slot] = defaultAction(kind)), { rerender: true }),
      ),
    ),
  );

  switch (action.kind) {
    case "midi_cc": {
      fs.append(field("CC #", numInput(action.cc, 0, MIDI_MAX, (v) => onEdit((b) => (b[slot].cc = v)))));
      fs.append(
        field(
          "Mode",
          selectInput(
            [
              { value: "fixed", label: "Fixed value" },
              { value: "toggle", label: "Toggle 0 / 127" },
            ],
            action.toggle ? "toggle" : "fixed",
            (mode) =>
              onEdit(
                (b) =>
                  (b[slot] =
                    mode === "toggle"
                      ? { kind: "midi_cc", cc: action.cc, toggle: true }
                      : { kind: "midi_cc", cc: action.cc, value: 0 }),
                { rerender: true },
              ),
          ),
        ),
      );
      if (!action.toggle) {
        fs.append(field("Value", numInput(action.value, 0, MIDI_MAX, (v) => onEdit((b) => (b[slot].value = v)))));
      }
      break;
    }
    case "program_change":
      fs.append(field("Program", numInput(action.program, 0, MIDI_MAX, (v) => onEdit((b) => (b[slot].program = v)))));
      break;
    case "sysex": {
      const meta = SYSEX_CMDS.find((s) => s.sysex === action.sysex) || SYSEX_CMDS[0];
      fs.append(
        field(
          "Command",
          selectInput(
            SYSEX_CMDS.map((s) => ({ value: s.sysex, label: s.label })),
            action.sysex,
            (sx) =>
              onEdit(
                (b) => {
                  const m = SYSEX_CMDS.find((s) => s.sysex === sx);
                  b[slot] = { kind: "sysex", sysex: sx, param: clamp(action.param, m.min, m.max) };
                },
                { rerender: true },
              ),
          ),
        ),
      );
      fs.append(field(`Param`, numInput(action.param, meta.min, meta.max, (v) => onEdit((b) => (b[slot].param = v))), el("span", { class: "hint" }, meta.hint)));
      break;
    }
    case "page_change": {
      const opts = ctx.pageNames.map((n, i) => ({ value: i, label: `Page ${i + 1}${n ? " — " + n : ""}` }));
      fs.append(field("Target", selectInput(opts, action.page, (v) => onEdit((b) => (b[slot].page = parseInt(v, 10))))));
      break;
    }
    // none / page_next / page_prev / tuner_toggle have no fields
  }
  return fs;
}

/**
 * @param {HTMLElement} container
 * @param {{page:object, index:number|null, pageCount:number, pageNames:string[]}} ctx
 * @param {(mutator:(btn:object)=>void, opts?:{rerender?:boolean})=>void} onEdit
 */
export function renderInspector(container, ctx, onEdit) {
  clear(container);
  if (ctx.index === null || ctx.index === undefined) {
    container.append(
      el("p", { class: "inspector-hint" }, "Select a switch on the board to edit its label, colour, and actions."),
    );
    return;
  }
  const b = ctx.page.buttons[ctx.index];
  container.append(el("h2", { class: "inspector-title" }, `Switch ${SLOT_NAMES[ctx.index]}`));

  // Label (with UTF-8 byte-limit warning — the firmware truncates at LABEL_CAP).
  const labelInput = el("input", { type: "text", value: b.label, class: "txt", maxlength: 48, placeholder: "(no label)" });
  const labelWarn = el("span", { class: "warn" });
  const refreshWarn = () => {
    labelWarn.textContent = byteLen(labelInput.value) > LABEL_CAP ? `truncates to ${LABEL_CAP} bytes on device` : "";
  };
  labelInput.addEventListener("input", () => {
    onEdit((btn) => (btn.label = labelInput.value));
    refreshWarn();
  });
  refreshWarn();
  container.append(field("Label", labelInput, labelWarn));

  // LED colour.
  const colorInput = el("input", { type: "color", value: colorToHex(b.color), class: "color" });
  colorInput.addEventListener("input", () => onEdit((btn) => (btn.color = hexToRgb(colorInput.value))));
  container.append(field("LED colour", colorInput, el("span", { class: "hint" }, "device LEDs are dim by design")));

  container.append(actionEditor("On press", "on_press", b.on_press, ctx, onEdit));
  container.append(actionEditor("On long-press", "on_long_press", b.on_long_press, ctx, onEdit));
}

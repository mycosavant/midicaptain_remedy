// RuntimeConfig model helpers — limits, defaults, normalization, validation,
// and UI metadata for the live-device editor. The *wire* layout lives in
// postcard.js; this module is the in-memory shape the UI edits.
//
// Limits mirror firmware/src/config/mod.rs:
//   MAX_PAGES = 8, PAGE_BUTTONS = 10, LABEL_CAP = 12 bytes, NAME_CAP = 16 bytes
// and the device clamps/truncates on its side too — we validate up front so the
// user sees problems before pushing.

import { PAGE_BUTTONS, ACTION_TAG, SYSEX_TAG } from "./postcard.js";

export { PAGE_BUTTONS };
export const MAX_PAGES = 8;
export const LABEL_CAP = 12; // bytes (UTF-8)
export const NAME_CAP = 16; // bytes (UTF-8)
export const MIDI_MAX = 127;

/**
 * Footswitch slot labels in scan-index order (config::SWITCH_FOR_BUTTON doc):
 * the buttons task emits SW1..SW4, A..D, UP, DOWN. Index i here is button i in
 * a page's `buttons[10]`.
 */
export const SLOT_NAMES = Object.freeze([
  "SW1", "SW2", "SW3", "SW4", "A", "B", "C", "D", "UP", "DOWN",
]);

/**
 * UI metadata for each Action kind: a human label and which extra fields the
 * inspector must show. Keys are the wire `kind` strings (postcard ACTION_TAG).
 */
export const ACTION_KINDS = Object.freeze([
  { kind: "none", label: "None", fields: [] },
  { kind: "midi_cc", label: "MIDI CC", fields: ["cc", "ccmode"] },
  { kind: "program_change", label: "Program Change", fields: ["program"] },
  { kind: "sysex", label: "Katana SysEx", fields: ["sysex", "param"] },
  { kind: "page_change", label: "Go to Page", fields: ["page"] },
  { kind: "page_next", label: "Next Page", fields: [] },
  { kind: "page_prev", label: "Previous Page", fields: [] },
  { kind: "tuner_toggle", label: "Tuner", fields: [] },
]);

/** Katana SysEx commands, with the valid `param` range for the inspector. */
export const SYSEX_CMDS = Object.freeze([
  { sysex: "recall_preset", label: "Recall Preset", min: 0, max: 4,
    hint: "0=Panel, 1-4=CH1-CH4" },
  { sysex: "amp_type", label: "Amp Type", min: 0, max: 4,
    hint: "0-4: Acoustic/Clean/Crunch/Lead/Brown" },
  { sysex: "gain", label: "Gain", min: 0, max: 100, hint: "0-100" },
  { sysex: "volume", label: "Volume", min: 0, max: 100, hint: "0-100" },
]);

const enc = new TextEncoder();
/** UTF-8 byte length of a string (the limit the firmware enforces). */
export function byteLen(s) {
  return enc.encode(s ?? "").length;
}

/** Truncate `s` to at most `cap` UTF-8 bytes on a char boundary. */
export function truncateToBytes(s, cap) {
  s = s ?? "";
  if (byteLen(s) <= cap) return s;
  let out = "";
  for (const ch of s) {
    if (byteLen(out + ch) > cap) break;
    out += ch;
  }
  return out;
}

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v | 0));

export function emptyButton() {
  return {
    label: "",
    color: [0, 0, 0],
    on_press: { kind: "none" },
    on_long_press: { kind: "none" },
  };
}

export function emptyPage(name = "") {
  return {
    name,
    buttons: Array.from({ length: PAGE_BUTTONS }, emptyButton),
  };
}

/** A minimal valid config for "start from scratch". */
export function emptyConfig() {
  return { pages: [emptyPage("PAGE 1")] };
}

/** Deep clone (configs are plain JSON-compatible objects). */
export function cloneConfig(cfg) {
  return JSON.parse(JSON.stringify(cfg));
}

/** Coerce an action object into a clean, in-range wire shape. */
function normalizeAction(a) {
  const kind = a && ACTION_TAG[a.kind] !== undefined ? a.kind : "none";
  switch (kind) {
    case "midi_cc":
      return a.toggle
        ? { kind, cc: clamp(a.cc, 0, MIDI_MAX), toggle: true }
        : { kind, cc: clamp(a.cc, 0, MIDI_MAX), value: clamp(a.value, 0, MIDI_MAX) };
    case "program_change":
      return { kind, program: clamp(a.program, 0, MIDI_MAX) };
    case "sysex": {
      const meta = SYSEX_CMDS.find((s) => s.sysex === a.sysex) || SYSEX_CMDS[0];
      return { kind, sysex: meta.sysex, param: clamp(a.param, meta.min, meta.max) };
    }
    case "page_change":
      return { kind, page: clamp(a.page, 0, MAX_PAGES - 1) };
    default:
      return { kind };
  }
}

/**
 * Return a normalized deep copy: exactly MAX_PAGES-bounded pages, each with a
 * truncated name and exactly 10 in-range buttons. Safe to encode + push.
 */
export function normalizeConfig(cfg) {
  const pages = ((cfg && cfg.pages) || []).slice(0, MAX_PAGES).map((p) => ({
    name: truncateToBytes(p?.name ?? "", NAME_CAP),
    buttons: Array.from({ length: PAGE_BUTTONS }, (_, i) => {
      const b = (p && p.buttons && p.buttons[i]) || emptyButton();
      const c = b.color || [0, 0, 0];
      return {
        label: truncateToBytes(b.label ?? "", LABEL_CAP),
        color: [clamp(c[0], 0, 255), clamp(c[1], 0, 255), clamp(c[2], 0, 255)],
        on_press: normalizeAction(b.on_press),
        on_long_press: normalizeAction(b.on_long_press),
      };
    }),
  }));
  if (pages.length === 0) pages.push(emptyPage("PAGE 1"));
  return { pages };
}

/**
 * Validate a config against the firmware's limits. Returns a list of problems;
 * empty means OK. (normalizeConfig fixes most of these, but validation lets the
 * UI warn rather than silently truncate.)
 */
export function validateConfig(cfg) {
  const errors = [];
  const pages = (cfg && cfg.pages) || [];
  if (pages.length === 0) errors.push({ path: "pages", msg: "at least one page required" });
  if (pages.length > MAX_PAGES)
    errors.push({ path: "pages", msg: `too many pages (max ${MAX_PAGES})` });
  pages.forEach((p, pi) => {
    if (byteLen(p?.name) > NAME_CAP)
      errors.push({ path: `pages[${pi}].name`, msg: `name over ${NAME_CAP} bytes` });
    const buttons = (p && p.buttons) || [];
    if (buttons.length !== PAGE_BUTTONS)
      errors.push({ path: `pages[${pi}].buttons`, msg: `must have ${PAGE_BUTTONS} buttons` });
    buttons.forEach((b, bi) => {
      if (byteLen(b?.label) > LABEL_CAP)
        errors.push({ path: `pages[${pi}].buttons[${bi}].label`, msg: `label over ${LABEL_CAP} bytes` });
    });
  });
  return { ok: errors.length === 0, errors };
}

/** A short, human description of an action for board/inspector summaries. */
export function describeAction(a) {
  if (!a || a.kind === "none") return "—";
  switch (a.kind) {
    case "midi_cc":
      return a.toggle ? `CC ${a.cc} toggle` : `CC ${a.cc} = ${a.value}`;
    case "program_change":
      return `PC ${a.program}`;
    case "sysex": {
      const meta = SYSEX_CMDS.find((s) => s.sysex === a.sysex);
      return `${meta ? meta.label : a.sysex} ${a.param}`;
    }
    case "page_change":
      return `→ Page ${a.page + 1}`;
    case "page_next":
      return "Next page";
    case "page_prev":
      return "Prev page";
    case "tuner_toggle":
      return "Tuner";
    default:
      return a.kind;
  }
}

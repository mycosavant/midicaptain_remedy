// postcard codec for `RuntimeConfig` — the byte layout the device speaks.
//
// This is a hand port of `encode_config` / `decode_config` in the reference
// host client (firmware/scripts/cdc_config_client.py), which is verified
// byte-exact against hardware and itself mirrors `firmware/src/config/mod.rs`.
// The three MUST stay in lockstep; webapp/test/codec.test.mjs guards it.
//
// Layout (postcard):
//   RuntimeConfig = varint(pages.len) ++ Page*
//   Page          = String(name) ++ Button[10]          // fixed array, no len
//   Button        = String(label) ++ u8 r ++ u8 g ++ u8 b ++ Action ++ Action
//   String        = varint(byteLen) ++ utf8 bytes
//   Action        = varint(tag) ++ <fields by variant>
//     none(0)            : —
//     midi_cc(1)         : u8 cc ++ CcValue
//     program_change(2)  : u8 program
//     sysex(3)           : varint(SysexCmd tag) ++ u8 param
//     page_change(4)     : u8 page
//     page_next(5) | page_prev(6) | tuner_toggle(7) : —
//   CcValue       = varint(tag) [++ u8 value]
//     fixed(0)  : u8 value
//     toggle(1) : —
//
// `tag`/length fields are LEB128 varints (postcard's rule); the value fields
// (cc, value, program, param, page, colour) are raw u8. All current values are
// < 128, so each is a single byte — identical to the device and the golden
// vectors — but the varint path keeps us correct if a variant set grows.

import { pushVarint, decodeVarint } from "./varint.js";

/** Footswitch slots per page — a fixed array, no length prefix on the wire. */
export const PAGE_BUTTONS = 10;

/** Action variant tags (must match config::Action declaration order). */
export const ACTION_TAG = Object.freeze({
  none: 0,
  midi_cc: 1,
  program_change: 2,
  sysex: 3,
  page_change: 4,
  page_next: 5,
  page_prev: 6,
  tuner_toggle: 7,
});
const ACTION_NAME = Object.freeze(invert(ACTION_TAG));

/** CcValue variant tags (config::CcValue). */
export const CC_FIXED = 0;
export const CC_TOGGLE = 1;

/** SysexCmd variant tags (config::SysexCmd). */
export const SYSEX_TAG = Object.freeze({
  recall_preset: 0,
  amp_type: 1,
  gain: 2,
  volume: 3,
});
const SYSEX_NAME = Object.freeze(invert(SYSEX_TAG));

function invert(obj) {
  const out = {};
  for (const k of Object.keys(obj)) out[obj[k]] = k;
  return out;
}

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder("utf-8", { fatal: false });

// ── encode ────────────────────────────────────────────────────────────────

function encStr(out, s) {
  const bytes = TEXT_ENCODER.encode(s ?? "");
  pushVarint(out, bytes.length);
  for (let i = 0; i < bytes.length; i++) out.push(bytes[i]);
}

function u8(out, v) {
  out.push((v | 0) & 0xff);
}

function encAction(out, a) {
  const kind = (a && a.kind) || "none";
  const tag = ACTION_TAG[kind];
  if (tag === undefined) throw new Error(`unknown action kind: ${kind}`);
  pushVarint(out, tag);
  switch (kind) {
    case "midi_cc":
      u8(out, a.cc);
      if (a.toggle) {
        pushVarint(out, CC_TOGGLE);
      } else {
        pushVarint(out, CC_FIXED);
        u8(out, a.value ?? 0);
      }
      break;
    case "program_change":
      u8(out, a.program);
      break;
    case "sysex": {
      const sx = SYSEX_TAG[a.sysex];
      if (sx === undefined) throw new Error(`unknown sysex cmd: ${a.sysex}`);
      pushVarint(out, sx);
      u8(out, a.param ?? 0);
      break;
    }
    case "page_change":
      u8(out, a.page ?? 0);
      break;
    // none / page_next / page_prev / tuner_toggle: tag only
  }
}

function encButton(out, b) {
  encStr(out, b?.label ?? "");
  const c = (b && b.color) || [0, 0, 0];
  u8(out, c[0]);
  u8(out, c[1]);
  u8(out, c[2]);
  encAction(out, b?.on_press ?? { kind: "none" });
  encAction(out, b?.on_long_press ?? { kind: "none" });
}

function encPage(out, p) {
  encStr(out, p?.name ?? "");
  const buttons = (p && p.buttons) || [];
  for (let i = 0; i < PAGE_BUTTONS; i++) encButton(out, buttons[i] || {});
}

/**
 * Serialize a RuntimeConfig (plain object, see runtimeconfig.js) to a postcard
 * blob.
 * @param {{pages: Array}} cfg
 * @returns {Uint8Array}
 */
export function encodeConfig(cfg) {
  const out = [];
  const pages = (cfg && cfg.pages) || [];
  pushVarint(out, pages.length);
  for (const p of pages) encPage(out, p);
  return Uint8Array.from(out);
}

// ── decode ──────────────────────────────────────────────────────────────

function decStr(data, i) {
  const [n, p] = decodeVarint(data, i);
  const slice = data.subarray(p, p + n);
  if (slice.length < n) throw new Error("postcard: string overruns input");
  return [TEXT_DECODER.decode(slice), p + n];
}

function decAction(data, i) {
  let p = i;
  let tag;
  [tag, p] = decodeVarint(data, p);
  const kind = ACTION_NAME[tag];
  if (kind === undefined) throw new Error(`postcard: bad action tag ${tag}`);
  switch (kind) {
    case "midi_cc": {
      const cc = data[p++];
      let mode;
      [mode, p] = decodeVarint(data, p);
      if (mode === CC_TOGGLE) return [{ kind, cc, toggle: true }, p];
      const value = data[p++];
      return [{ kind, cc, value }, p];
    }
    case "program_change":
      return [{ kind, program: data[p++] }, p];
    case "sysex": {
      let sx;
      [sx, p] = decodeVarint(data, p);
      const name = SYSEX_NAME[sx];
      if (name === undefined) throw new Error(`postcard: bad sysex tag ${sx}`);
      return [{ kind, sysex: name, param: data[p++] }, p];
    }
    case "page_change":
      return [{ kind, page: data[p++] }, p];
    default:
      return [{ kind }, p];
  }
}

function decButton(data, i) {
  let p = i;
  let label;
  [label, p] = decStr(data, p);
  const color = [data[p], data[p + 1], data[p + 2]];
  p += 3;
  let on_press, on_long_press;
  [on_press, p] = decAction(data, p);
  [on_long_press, p] = decAction(data, p);
  return [{ label, color, on_press, on_long_press }, p];
}

function decPage(data, i) {
  let p = i;
  let name;
  [name, p] = decStr(data, p);
  const buttons = [];
  for (let b = 0; b < PAGE_BUTTONS; b++) {
    let btn;
    [btn, p] = decButton(data, p);
    buttons.push(btn);
  }
  return [{ name, buttons }, p];
}

/**
 * Deserialize a postcard blob into a RuntimeConfig object.
 * @param {Uint8Array} data
 * @returns {{config: {pages: Array}, consumed: number}}
 */
export function decodeConfig(data) {
  const bytes = data instanceof Uint8Array ? data : Uint8Array.from(data);
  let p = 0;
  let npages;
  [npages, p] = decodeVarint(bytes, p);
  const pages = [];
  for (let i = 0; i < npages; i++) {
    let page;
    [page, p] = decPage(bytes, p);
    pages.push(page);
  }
  return { config: { pages }, consumed: p };
}

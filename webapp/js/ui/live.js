// Live Device editor controller: owns the in-memory RuntimeConfig and wires the
// connection bar, page bar, board, and inspector to the CdcLink (Web Serial).
//
// Flow: Connect -> HELLO (version check) -> GET_CONFIG -> edit -> Write
// (SET_CONFIG, which the device persists + hot-reloads before replying).

import { CdcLink } from "../device/protocol.js";
import {
  emptyConfig,
  emptyPage,
  normalizeConfig,
  validateConfig,
  MAX_PAGES,
  NAME_CAP,
  byteLen,
} from "../device/runtimeconfig.js";
import { el, clear } from "./dom.js";
import { renderBoard } from "./board.js";
import { renderInspector } from "./inspector.js";

export function createLiveEditor(root) {
  const state = {
    link: new CdcLink(),
    config: emptyConfig(),
    pageIndex: 0,
    buttonIndex: null,
    connected: false,
    deviceVersion: null,
    dirty: false,
  };
  const els = {};

  // ── skeleton ──
  clear(root);
  els.conn = el("div", { class: "conn-bar" });
  els.pageBar = el("div", { class: "page-bar" });
  els.board = el("div", { class: "board" });
  els.inspector = el("aside", { class: "inspector" });
  els.status = el("div", { class: "status-line" });
  root.append(
    els.conn,
    els.pageBar,
    el("div", { class: "editor" }, els.board, els.inspector),
    els.status,
  );

  // ── helpers ──
  const activePage = () => state.config.pages[state.pageIndex];
  const pageTabLabel = (i, p) => `${i + 1}${p.name ? ": " + p.name : ""}`;

  function setStatus(msg, kind = "info") {
    clear(els.status);
    els.status.className = "status-line " + kind;
    els.status.append(msg);
  }

  // ── connection bar ──
  function renderConn() {
    clear(els.conn);
    if (!CdcLink.isSupported()) {
      els.conn.append(
        el("span", { class: "badge err" }, "Web Serial unavailable"),
        el("span", { class: "hint" }, "Open in Chrome or Edge over https or localhost (not file://)."),
      );
      return;
    }
    if (!state.connected) {
      els.conn.append(
        el("button", { class: "btn primary", type: "button", onClick: connect }, "Connect device"),
        el("span", { class: "hint" }, "Plug in the MIDI Captain, then Connect."),
      );
      return;
    }
    const writeBtn = el("button", { class: "btn primary", type: "button", onClick: writeToDevice }, "Write to device");
    if (!state.dirty) writeBtn.disabled = true;
    els.conn.append(
      el("span", { class: "badge ok" }, `Connected · proto v${state.deviceVersion}`),
      el("button", { class: "btn", type: "button", onClick: readFromDevice }, "Read"),
      writeBtn,
      el("button", { class: "btn", type: "button", onClick: disconnect }, "Disconnect"),
      state.dirty ? el("span", { class: "badge warn" }, "unsaved edits") : null,
    );
  }

  // ── page bar ──
  function renderPageBar() {
    clear(els.pageBar);
    const tabs = el("div", { class: "page-tabs" });
    state.config.pages.forEach((p, i) => {
      tabs.append(
        el(
          "button",
          { class: "page-tab" + (i === state.pageIndex ? " active" : ""), type: "button", onClick: () => selectPage(i) },
          pageTabLabel(i, p),
        ),
      );
    });

    const nameInput = el("input", {
      type: "text",
      value: activePage().name,
      class: "txt page-name",
      maxlength: 48,
      placeholder: "page name",
    });
    const nameWarn = el("span", { class: "warn" });
    const refreshNameWarn = () => {
      nameWarn.textContent = byteLen(nameInput.value) > NAME_CAP ? `truncates to ${NAME_CAP} bytes` : "";
    };
    nameInput.addEventListener("input", () => {
      activePage().name = nameInput.value;
      markDirty();
      const tab = tabs.children[state.pageIndex];
      if (tab) tab.textContent = pageTabLabel(state.pageIndex, activePage());
      refreshNameWarn();
    });
    refreshNameWarn();

    const addBtn = el("button", { class: "btn small", type: "button", onClick: addPage }, "+ Page");
    if (state.config.pages.length >= MAX_PAGES) addBtn.disabled = true;
    const delBtn = el("button", { class: "btn small", type: "button", onClick: removePage }, "− Page");
    if (state.config.pages.length <= 1) delBtn.disabled = true;

    els.pageBar.append(
      tabs,
      el(
        "div",
        { class: "page-ctrls" },
        el("label", { class: "field inline" }, el("span", { class: "field-name" }, "Name"), nameInput, nameWarn),
        addBtn,
        delBtn,
      ),
    );
  }

  // ── board + inspector ──
  function renderBoardOnly() {
    renderBoard(els.board, activePage(), state.buttonIndex, selectButton);
  }
  function renderInspectorOnly() {
    renderInspector(
      els.inspector,
      {
        page: activePage(),
        index: state.buttonIndex,
        pageCount: state.config.pages.length,
        pageNames: state.config.pages.map((p) => p.name),
      },
      onEdit,
    );
  }
  function renderAll() {
    renderPageBar();
    renderBoardOnly();
    renderInspectorOnly();
  }

  function markDirty() {
    if (!state.dirty) {
      state.dirty = true;
      renderConn();
    }
  }

  // ── interactions ──
  function selectButton(i) {
    state.buttonIndex = i;
    renderBoardOnly();
    renderInspectorOnly();
  }
  function selectPage(i) {
    state.pageIndex = i;
    state.buttonIndex = null;
    renderAll();
  }
  function addPage() {
    if (state.config.pages.length >= MAX_PAGES) return;
    state.config.pages.push(emptyPage(`PAGE ${state.config.pages.length + 1}`));
    state.pageIndex = state.config.pages.length - 1;
    state.buttonIndex = null;
    markDirty();
    renderAll();
  }
  function removePage() {
    if (state.config.pages.length <= 1) return;
    if (!confirm(`Delete page ${state.pageIndex + 1}?`)) return;
    state.config.pages.splice(state.pageIndex, 1);
    state.pageIndex = Math.min(state.pageIndex, state.config.pages.length - 1);
    state.buttonIndex = null;
    markDirty();
    renderAll();
  }

  // Called by the inspector for every edit.
  function onEdit(mutator, { rerender = false } = {}) {
    mutator(activePage().buttons[state.buttonIndex]);
    markDirty();
    renderBoardOnly();
    if (rerender) renderInspectorOnly();
  }

  // ── device I/O ──
  function loadConfig(cfg) {
    state.config = normalizeConfig(cfg);
    state.pageIndex = 0;
    state.buttonIndex = null;
    state.dirty = false;
    renderAll();
    renderConn();
  }

  async function connect() {
    try {
      state.link.onClose = () => {
        state.connected = false;
        state.deviceVersion = null;
        renderConn();
        setStatus("Device disconnected.", "warn");
      };
      setStatus("Select the device's serial port…");
      await state.link.connect();
      setStatus("Handshaking…");
      state.deviceVersion = await state.link.hello();
      state.connected = true;
      setStatus("Reading configuration from device…");
      loadConfig(await state.link.getConfig());
      setStatus(`Connected. Loaded ${state.config.pages.length} page(s) from device.`, "ok");
    } catch (e) {
      setStatus("Connect failed: " + e.message, "err");
      try {
        await state.link.disconnect();
      } catch {
        /* ignore */
      }
      state.connected = false;
      renderConn();
    }
  }

  async function readFromDevice() {
    if (state.dirty && !confirm("Discard unsaved edits and re-read the device's config?")) return;
    try {
      setStatus("Reading…");
      loadConfig(await state.link.getConfig());
      setStatus("Configuration read from device.", "ok");
    } catch (e) {
      setStatus("Read failed: " + e.message, "err");
    }
  }

  async function writeToDevice() {
    const norm = normalizeConfig(state.config);
    const { ok, errors } = validateConfig(norm);
    if (!ok) {
      setStatus(`Cannot push: ${errors[0].path} — ${errors[0].msg}`, "err");
      return;
    }
    try {
      setStatus("Writing to device…");
      await state.link.setConfig(norm);
      state.config = norm;
      state.dirty = false;
      renderAll();
      renderConn();
      setStatus("Pushed — device persisted to flash and hot-reloaded. Change is live.", "ok");
    } catch (e) {
      setStatus("Write failed: " + e.message, "err");
    }
  }

  async function disconnect() {
    try {
      await state.link.disconnect();
    } catch {
      /* ignore */
    }
    state.connected = false;
    state.deviceVersion = null;
    renderConn();
    setStatus("Disconnected.", "info");
  }

  // ── boot ──
  renderConn();
  renderAll();
  setStatus(
    CdcLink.isSupported()
      ? "Not connected — editing a blank config. Connect to load the device's live config."
      : "Read-only preview — Web Serial isn't available in this browser.",
  );

  return { state };
}

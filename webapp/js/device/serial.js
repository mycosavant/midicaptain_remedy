// Web Serial transport for the device's USB-CDC port. Owns the port, a
// background read loop that splits the byte stream on the 0x00 frame delimiter,
// and a frame queue the protocol layer pulls from. No codec knowledge here.
//
// Web Serial is Chromium-only and requires a secure context (https or
// localhost). The device enumerates as VID:PID 2E8A:102D.

import { DELIMITER } from "./frame.js";

/** USB identity of the MIDI Captain Remedy CDC interface. */
export const DEVICE_FILTERS = [{ usbVendorId: 0x2e8a, usbProductId: 0x102d }];

export class CdcSerial {
  constructor() {
    this.port = null;
    this._reader = null;
    this._writer = null;
    this._readTask = null;
    this._buf = []; // accumulated bytes for the current (incomplete) frame
    this._frames = []; // complete COBS frames (delimiter stripped) awaiting consumption
    this._waiters = []; // pending nextFrame() resolvers
    /** Called when the device goes away mid-session. */
    this.onClose = null;
  }

  static isSupported() {
    return typeof navigator !== "undefined" && "serial" in navigator;
  }

  /** Prompt the user to pick the device port, then open it. */
  async requestAndOpen({ baudRate = 115200, filters = DEVICE_FILTERS } = {}) {
    if (!CdcSerial.isSupported()) {
      throw new Error("Web Serial is not available — use Chrome/Edge over https or localhost.");
    }
    const port = await navigator.serial.requestPort({ filters });
    await this.openPort(port, baudRate);
  }

  /** Open a port object (e.g. one returned by navigator.serial.getPorts()). */
  async openPort(port, baudRate = 115200) {
    await port.open({ baudRate });
    this.port = port;
    this._writer = port.writable.getWriter();
    this._reader = port.readable.getReader();
    this._readTask = this._readLoop();
  }

  get isOpen() {
    return this.port !== null;
  }

  async _readLoop() {
    try {
      for (;;) {
        const { value, done } = await this._reader.read();
        if (done) break;
        if (value) this._ingest(value);
      }
    } catch {
      // read errored (device unplugged / port closed) — fall through to cleanup
    } finally {
      this._teardown();
    }
  }

  _ingest(chunk) {
    for (let i = 0; i < chunk.length; i++) {
      const b = chunk[i];
      if (b === DELIMITER) {
        if (this._buf.length > 0) {
          this._deliver(Uint8Array.from(this._buf));
          this._buf = [];
        }
        // a lone delimiter (idle/keepalive) just resets framing
      } else {
        this._buf.push(b);
      }
    }
  }

  _deliver(frame) {
    const waiter = this._waiters.shift();
    if (waiter) {
      clearTimeout(waiter.timer);
      waiter.resolve(frame);
    } else {
      this._frames.push(frame);
    }
  }

  /** Discard any buffered/queued frames (parallel to pyserial reset_input_buffer). */
  drain() {
    this._frames.length = 0;
    this._buf = [];
  }

  /**
   * Resolve with the next complete frame (COBS bytes, delimiter stripped), or
   * reject after `timeoutMs`.
   * @returns {Promise<Uint8Array>}
   */
  nextFrame(timeoutMs = 2000) {
    const queued = this._frames.shift();
    if (queued) return Promise.resolve(queued);
    return new Promise((resolve, reject) => {
      const waiter = { resolve, reject, timer: null };
      waiter.timer = setTimeout(() => {
        const idx = this._waiters.indexOf(waiter);
        if (idx >= 0) this._waiters.splice(idx, 1);
        reject(new Error("timed out waiting for device reply"));
      }, timeoutMs);
      this._waiters.push(waiter);
    });
  }

  async write(bytes) {
    if (!this._writer) throw new Error("port not open");
    await this._writer.write(bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes));
  }

  _teardown() {
    const wasOpen = this.port !== null;
    this.port = null;
    this._reader = null;
    this._writer = null;
    for (const w of this._waiters.splice(0)) {
      clearTimeout(w.timer);
      w.reject(new Error("port closed"));
    }
    if (wasOpen && typeof this.onClose === "function") this.onClose();
  }

  async close() {
    const port = this.port;
    try {
      if (this._reader) await this._reader.cancel().catch(() => {});
      if (this._writer) {
        await this._writer.close().catch(() => {});
        this._writer.releaseLock?.();
      }
    } finally {
      if (this._readTask) await this._readTask.catch(() => {});
      try {
        if (port) await port.close();
      } catch {
        /* already closing */
      }
      this._teardown();
    }
  }
}

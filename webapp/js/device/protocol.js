// CdcLink — the request/response protocol client over the CDC transport.
// Ports `hello` / `get_config` / `set_config` from the reference host client
// (firmware/scripts/cdc_config_client.py) onto Web Serial.

import { CdcSerial } from "./serial.js";
import { CMD, PROTO_ERROR, encodeFrame, decodeFrame } from "./frame.js";
import { encodeConfig, decodeConfig } from "./postcard.js";

/** Wire-protocol version this editor speaks (firmware proto::PROTO_VERSION). */
export const PROTO_VERSION = 1;

/** An ERROR reply from the device, or a protocol-level mismatch. */
export class ProtocolError extends Error {
  constructor(message, { code = null, deviceVersion = null } = {}) {
    super(message);
    this.name = "ProtocolError";
    this.code = code; // numeric ProtoError, if from an ERROR frame
    this.deviceVersion = deviceVersion;
  }
}

export class CdcLink {
  constructor() {
    this.serial = new CdcSerial();
    this._seq = 0;
    this.deviceVersion = null;
  }

  static isSupported() {
    return CdcSerial.isSupported();
  }

  get isOpen() {
    return this.serial.isOpen;
  }

  /** Set a callback for unexpected disconnects. */
  set onClose(fn) {
    this.serial.onClose = fn;
  }

  /** Prompt for the port and open it (does NOT handshake — call hello() next). */
  async connect() {
    await this.serial.requestAndOpen();
  }

  async disconnect() {
    await this.serial.close();
  }

  _nextSeq() {
    this._seq = (this._seq + 1) & 0xff;
    return this._seq;
  }

  async _transact(cmd, payload, { timeoutMs = 2000, seq = this._nextSeq() } = {}) {
    if (!this.isOpen) throw new Error("not connected");
    this.serial.drain();
    await this.serial.write(encodeFrame(cmd, seq, payload));
    const reply = decodeFrame(await this.serial.nextFrame(timeoutMs));
    if (reply.cmd === CMD.ERROR) {
      const code = reply.payload[0];
      throw new ProtocolError(
        `device error: ${PROTO_ERROR[code] || `code ${code}`}`,
        { code },
      );
    }
    return reply;
  }

  /** Handshake. Verifies the device speaks the same protocol version. */
  async hello({ timeoutMs = 2000 } = {}) {
    const reply = await this._transact(CMD.HELLO, Uint8Array.of(PROTO_VERSION), {
      seq: 0x42,
      timeoutMs,
    });
    if (reply.cmd !== CMD.HELLO) {
      throw new ProtocolError(`unexpected HELLO reply (cmd 0x${reply.cmd.toString(16)})`);
    }
    const version = reply.payload[0];
    this.deviceVersion = version;
    if (version !== PROTO_VERSION) {
      throw new ProtocolError(
        `protocol mismatch: device speaks v${version}, this editor speaks v${PROTO_VERSION}. Update whichever is older.`,
        { deviceVersion: version },
      );
    }
    return version;
  }

  /** Read the device's live config. */
  async getConfig({ timeoutMs = 3000 } = {}) {
    const reply = await this._transact(CMD.GET_CONFIG, new Uint8Array(0), { timeoutMs });
    if (reply.cmd !== CMD.GET_CONFIG) {
      throw new ProtocolError(`unexpected GET reply (cmd 0x${reply.cmd.toString(16)})`);
    }
    const { config, consumed } = decodeConfig(reply.payload);
    if (consumed !== reply.payload.length) {
      throw new ProtocolError("GET reply had trailing bytes (codec out of sync?)");
    }
    return config;
  }

  /**
   * Push a config. The device validates, persists to flash, and hot-reloads
   * before replying — on success the change is already live.
   */
  async setConfig(cfg, { timeoutMs = 4000 } = {}) {
    const blob = encodeConfig(cfg);
    const reply = await this._transact(CMD.SET_CONFIG, blob, { timeoutMs });
    if (reply.cmd !== CMD.SET_CONFIG) {
      throw new ProtocolError(`unexpected SET reply (cmd 0x${reply.cmd.toString(16)})`);
    }
    return true;
  }
}

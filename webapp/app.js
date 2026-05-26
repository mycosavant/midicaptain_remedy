/* ============================================================ *
 *  MIDI Captain Preset Builder -- vanilla JS, no deps.           *
 *                                                                *
 *  Sections:                                                     *
 *    1. Constants                                                *
 *    2. Default state (built-in presets)                         *
 *    3. Persistence (localStorage)                               *
 *    4. Serializers (SuperMode + GeekMode)                       *
 *    5. Minimal STORED-method ZIP writer                         *
 *    6. Rendering                                                *
 *    7. Event wiring                                             *
 *    8. Init                                                     *
 * ============================================================ */

// ---------- 0. i18n ----------

const I18N = {
  en: {
    brand: 'MIDI CAPTAIN / PRESET BUILDER',
    pages: 'PAGES', new: '+ NEW', dup: 'DUP', del: 'DEL',
    page_config: 'PAGE CONFIG', name: 'NAME', name_hint: '≤4 char',
    enc_cc: 'ENCODER CC', enc_label: 'ENCODER LABEL', midi_thru: 'MIDI THROUGH',
    exp1: 'EXP1 CH/CC', exp2: 'EXP2 CH/CC',
    display_mode: 'DISPLAY MODE', group_number: 'GROUP NUMBER',
    button_grid: 'BUTTON GRID', click_to_edit: 'click to edit',
    label: 'LABEL', label_hint: '≤4 char, SuperMode only',
    led_color: 'LED COLOR', led_mode: 'LED MODE', keytimes: 'KEYTIMES',
    keytimes_hint: 'tap states', state_n: 'STATE',
    on_press: 'ON PRESS', on_release: 'ON RELEASE',
    on_long: 'ON LONG PRESS', on_long_rel: 'ON LONG RELEASE',
    add: '+ ADD', set: '+ SET',
    preview: 'PREVIEW', copy: 'COPY', download: 'DOWNLOAD', dl_all: 'DL ALL',
    import: 'IMPORT',
    reference: 'REFERENCE', sources: 'SOURCES',
    footer_storage: 'state lives in localStorage — clears never unless you do',
    footer_tech: 'vanilla JS, zero deps', footer_source: 'source',
  },
  zh: {
    brand: 'MIDI CAPTAIN / 预设编辑器',
    pages: '页面', new: '+ 新建', dup: '复制', del: '删除',
    page_config: '页面设置', name: '名称', name_hint: '≤4 字符',
    enc_cc: '编码器 CC', enc_label: '编码器名', midi_thru: 'MIDI 转发',
    exp1: 'EXP1 通道/CC', exp2: 'EXP2 通道/CC',
    display_mode: '显示模式', group_number: '分组数',
    button_grid: '按钮网格', click_to_edit: '点击编辑',
    label: '标签', label_hint: '≤4 字符,仅 SuperMode',
    led_color: 'LED 颜色', led_mode: 'LED 模式', keytimes: '点击次数',
    keytimes_hint: '点击状态', state_n: '状态',
    on_press: '按下时', on_release: '释放时',
    on_long: '长按时', on_long_rel: '长释放',
    add: '+ 添加', set: '+ 设置',
    preview: '预览', copy: '复制', download: '下载', dl_all: '全部下载',
    import: '导入',
    reference: '参考', sources: '来源',
    footer_storage: '状态保存于本地存储，除非手动清除否则保留',
    footer_tech: '原生 JS, 零依赖', footer_source: '源码',
  },
  fr: {
    brand: 'MIDI CAPTAIN / ÉDITEUR DE PRÉRÉGLAGES',
    pages: 'PAGES', new: '+ NOUV.', dup: 'DUP.', del: 'SUPPR.',
    page_config: 'CONFIG. PAGE', name: 'NOM', name_hint: '≤4 car.',
    enc_cc: 'CC ENCODEUR', enc_label: 'NOM ENCODEUR', midi_thru: 'MIDI THROUGH',
    exp1: 'EXP1 CH/CC', exp2: 'EXP2 CH/CC',
    display_mode: 'AFFICHAGE', group_number: 'GROUPE',
    button_grid: 'GRILLE', click_to_edit: 'cliquer pour modifier',
    label: 'ÉTIQ.', label_hint: '≤4 car., SuperMode seul.',
    led_color: 'COULEUR LED', led_mode: 'MODE LED', keytimes: 'APPUIS',
    keytimes_hint: 'états', state_n: 'ÉTAT',
    on_press: 'APPUI', on_release: 'RELÂCHE',
    on_long: 'APPUI LONG', on_long_rel: 'RELÂCHE LONG',
    add: '+ AJOUT', set: '+ DÉFINIR',
    preview: 'APERÇU', copy: 'COPIER', download: 'TÉLÉCH.', dl_all: 'TOUT',
    import: 'IMPORTER',
    reference: 'RÉFÉRENCE', sources: 'SOURCES',
    footer_storage: 'l’état est conservé dans localStorage — ne s’efface que par vous',
    footer_tech: 'JS pur, zéro dépendance', footer_source: 'source',
  },
  es: {
    brand: 'MIDI CAPTAIN / EDITOR DE PRESETS',
    pages: 'PÁGINAS', new: '+ NUEVO', dup: 'DUP.', del: 'BORRAR',
    page_config: 'CONFIG. PÁGINA', name: 'NOMBRE', name_hint: '≤4 car.',
    enc_cc: 'CC ENCODER', enc_label: 'ETIQ. ENCODER', midi_thru: 'MIDI THRU',
    exp1: 'EXP1 CH/CC', exp2: 'EXP2 CH/CC',
    display_mode: 'PANTALLA', group_number: 'GRUPO',
    button_grid: 'CUADRÍCULA', click_to_edit: 'clic para editar',
    label: 'ETIQUETA', label_hint: '≤4 car., sólo SuperMode',
    led_color: 'COLOR LED', led_mode: 'MODO LED', keytimes: 'PULSACIONES',
    keytimes_hint: 'estados', state_n: 'ESTADO',
    on_press: 'AL PULSAR', on_release: 'AL SOLTAR',
    on_long: 'PULSACIÓN LARGA', on_long_rel: 'LIBER. LARGA',
    add: '+ AÑADIR', set: '+ DEFINIR',
    preview: 'VISTA PREVIA', copy: 'COPIAR', download: 'DESCARGAR', dl_all: 'TODO',
    import: 'IMPORTAR',
    reference: 'REFERENCIA', sources: 'FUENTES',
    footer_storage: 'el estado vive en localStorage — solo se borra si lo haces',
    footer_tech: 'JS puro, sin dependencias', footer_source: 'código',
  },
  pt: {
    brand: 'MIDI CAPTAIN / EDITOR DE PRESETS',
    pages: 'PÁGINAS', new: '+ NOVO', dup: 'DUP.', del: 'APAGAR',
    page_config: 'CONFIG. PÁGINA', name: 'NOME', name_hint: '≤4 car.',
    enc_cc: 'CC ENCODER', enc_label: 'ETIQ. ENCODER', midi_thru: 'MIDI THRU',
    exp1: 'EXP1 CH/CC', exp2: 'EXP2 CH/CC',
    display_mode: 'TELA', group_number: 'GRUPO',
    button_grid: 'GRADE', click_to_edit: 'clique para editar',
    label: 'RÓTULO', label_hint: '≤4 car., só SuperMode',
    led_color: 'COR LED', led_mode: 'MODO LED', keytimes: 'TOQUES',
    keytimes_hint: 'estados', state_n: 'ESTADO',
    on_press: 'AO PRESSIONAR', on_release: 'AO SOLTAR',
    on_long: 'PRESS. LONGA', on_long_rel: 'SOLTAR LONGO',
    add: '+ ADICIONAR', set: '+ DEFINIR',
    preview: 'PRÉVIA', copy: 'COPIAR', download: 'BAIXAR', dl_all: 'TUDO',
    import: 'IMPORTAR',
    reference: 'REFERÊNCIA', sources: 'FONTES',
    footer_storage: 'estado no localStorage — só limpa se você quiser',
    footer_tech: 'JS puro, zero dependências', footer_source: 'código',
  },
};

// ---------- 1. constants ----------

const STORAGE_KEY = 'mc-preset-builder-v1';
const ACTION_TYPES = ['CC', 'PC', 'NT', 'UP', 'DW', '--'];
const TYPE_DESCRIPTIONS = {
  CC: 'Control Change',
  PC: 'Program Change',
  NT: 'Note On',
  UP: 'Page Up (geek)',
  DW: 'Page Down (geek)',
  '--': 'Disabled',
};

// GeekMode palette (best-effort inferred from OEM defaults; verify on device).
const PALETTE = [
  ['#1a1a1a', 'off/dim'],
  ['#ff0000', 'red'],
  ['#00ff00', 'green'],
  ['#0040ff', 'blue'],
  ['#ffff00', 'yellow'],
  ['#ff00ff', 'magenta'],
  ['#ff8000', 'orange'],
  ['#8000ff', 'purple'],
  ['#00ffff', 'cyan'],
  ['#ff5599', 'pink'],
  ['#ff4444', 'mix 10'],
  ['#44ff44', 'mix 11'],
  ['#4444ff', 'mix 12'],
  ['#ffaa00', 'mix 13'],
  ['#aa00ff', 'mix 14'],
  ['#ffe6cc', 'warm white'],
  ['#aaffaa', 'mix 16'],
  ['#aaaaff', 'mix 17'],
  ['#ccddff', 'cool white'],
  ['#ffaacc', 'mix 19'],
  ['#aaffff', 'mix 20'],
  ['#ffffaa', 'mix 21'],
];

const KEY_HINTS = [
  'Top-left switch',
  'Top row · pos 2',
  'Top row · pos 3',
  'Top row · pos 4',
  'Top-right · usually BANK+ / UP',
  'Bottom-left switch',
  'Bottom row · pos 2',
  'Bottom row · pos 3',
  'Bottom row · pos 4 · usually TAP',
  'Bottom-right · usually BANK- / DW',
];

const DEVICE_FAMILIES = {
  std:      { name: 'Standard',  keys: 10, cols: 5, rows: 2 },
  goldblue: { name: 'Blue/Gold', keys: 10, cols: 5, rows: 2 },
  mini:     { name: 'Mini',      keys: 6,  cols: 3, rows: 2 },
  nano:     { name: 'Nano',      keys: 4,  cols: 4, rows: 1 },
};

// ---------- 2. default state ----------

const cc = (ch, num, val) => ({ type: 'CC', ch, p1: num, p2: val });
const pc = (ch, num) => ({ type: 'PC', ch, p1: num, p2: 0 });
const nt = (ch, num, vel) => ({ type: 'NT', ch, p1: num, p2: vel });
const bankInc = () => ({ type: 'PC', ch: 1, p1: -1, p2: 0, _bank: 'inc' });
const bankDec = () => ({ type: 'PC', ch: 1, p1: -1, p2: 0, _bank: 'dec' });

function mkKey(label, color, palette, opts = {}) {
  const baseColor = color != null ? color : 0x222222;
  return {
    label: label || '',
    color: baseColor,                              // legacy single color (still supported by emitter as fallback)
    colors: opts.colors || [baseColor],            // per-state colors; colors[i] applies to state i+1
    palette: palette != null ? palette : 0,
    keytimes: opts.keytimes || 1,
    ledmode: opts.ledmode || 'normal',
    press: opts.press || [],                       // press[i] = state i+1's press action (SuperMode), or stacked action (GeekMode)
    release: opts.release || [],
    long: Array.isArray(opts.long) ? opts.long : (opts.long ? [opts.long] : []),                  // per-state long actions
    long_release: Array.isArray(opts.long_release) ? opts.long_release : (opts.long_release ? [opts.long_release] : []),
  };
}

// Migrate old saved-state shape (single color, single long action) -> new shape.
function migrateKey(k) {
  if (k.colors === undefined) k.colors = [k.color != null ? k.color : 0xffffff];
  if (!Array.isArray(k.long)) k.long = k.long ? [k.long] : [];
  if (k.long_release === undefined) k.long_release = [];
  if (!Array.isArray(k.long_release)) k.long_release = k.long_release ? [k.long_release] : [];
  return k;
}

function mkPage(name, target, keys, meta = {}) {
  return {
    name,
    target,
    encoder_cc: meta.encoder_cc ?? 7,
    encoder_name: meta.encoder_name ?? 'VOL',
    exp1_ch: meta.exp1_ch ?? 1,
    exp1_cc: meta.exp1_cc ?? 1,
    exp2_ch: meta.exp2_ch ?? 1,
    exp2_cc: meta.exp2_cc ?? 7,
    midithrough: meta.midithrough ?? true,
    display_abc: meta.display_abc ?? 'abc4',
    group_number: meta.group_number ?? 4,
    pc_offset: meta.pc_offset ?? 1,
    bank_offset: meta.bank_offset ?? 1,
    keys: keys || {},
  };
}

function defaultState() {
  const katana = mkPage('KTNA', 'BOSS Katana', {
    0: mkKey('BOOST', 0x00ff00, 2, { keytimes: 2, press: [cc(1,16,127)], release: [cc(1,16,0)] }),
    1: mkKey('MOD',   0x0080ff, 3, { keytimes: 2, press: [cc(1,17,127)], release: [cc(1,17,0)] }),
    2: mkKey('DELAY', 0xffaa00, 6, { keytimes: 2, press: [cc(1,19,127)], release: [cc(1,19,0)] }),
    3: mkKey('REVRB', 0xaa00ff, 7, { keytimes: 2, press: [cc(1,20,127)], release: [cc(1,20,0)] }),
    4: mkKey('BANK+', 0xffffff, 0, { press: [bankInc()] }),
    5: mkKey('CH1',   0x00ffff, 8, { ledmode: 'select', press: [pc(1,0)] }),
    6: mkKey('CH2',   0x00ffff, 8, { ledmode: 'select', press: [pc(1,1)] }),
    7: mkKey('CH3',   0x00ffff, 8, { ledmode: 'select', press: [pc(1,2)] }),
    8: mkKey('TAP',   0xff0000, 1, { ledmode: 'tap', press: [cc(1,64,64)] }),
    9: mkKey('BANK-', 0xffffff, 0, { press: [bankDec()] }),
  }, { encoder_cc: 7, encoder_name: 'VOL', exp1_cc: 1, exp2_cc: 7 });

  const ndsp = mkPage('NDSP', 'Neural DSP Archetype', {
    0: mkKey('BYPS', 0x660000, 1, { ledmode: 'select', press: [cc(1,100,0)] }),
    1: mkKey('CHN1', 0x00ff00, 2, { ledmode: 'select', press: [cc(1,100,1)] }),
    2: mkKey('CHN2', 0xffaa00, 6, { ledmode: 'select', press: [cc(1,100,2)] }),
    3: mkKey('CHN3', 0xff0000, 1, { ledmode: 'select', press: [cc(1,100,3)] }),
    4: mkKey('BANK+', 0xffffff, 0, { press: [bankInc()] }),
    5: mkKey('ST1',  0x00ffff, 8, { keytimes: 2, press: [cc(1,102,127)], release: [cc(1,102,0)] }),
    6: mkKey('ST2',  0xff00ff, 5, { keytimes: 2, press: [cc(1,103,127)], release: [cc(1,103,0)] }),
    7: mkKey('ST3',  0xffff00, 4, { keytimes: 2, press: [cc(1,104,127)], release: [cc(1,104,0)] }),
    8: mkKey('TAP',  0xff0000, 1, { ledmode: 'tap', press: [cc(1,64,64)] }),
    9: mkKey('BANK-', 0xffffff, 0, { press: [bankDec()] }),
  }, { encoder_name: 'OUT', exp1_cc: 11, exp2_cc: 7 });

  const helix = mkPage('HXLP', 'Helix Native + Looper', {
    0: mkKey('SNP1', 0x00ff00, 2, { ledmode: 'select', press: [cc(1,69,0)] }),
    1: mkKey('SNP2', 0xffaa00, 6, { ledmode: 'select', press: [cc(1,69,1)] }),
    2: mkKey('SNP3', 0xff0000, 1, { ledmode: 'select', press: [cc(1,69,2)] }),
    3: mkKey('SNP4', 0xaa00ff, 7, { ledmode: 'select', press: [cc(1,69,3)] }),
    4: mkKey('TUNR', 0xffffff, 15,{ keytimes: 2, press: [cc(1,68,127)], release: [cc(1,68,0)] }),
    5: mkKey('REC',  0xff0000, 1, { press: [cc(1,60,127)] }),
    6: mkKey('PLAY', 0x00ff00, 2, { keytimes: 2, press: [cc(1,61,127)], release: [cc(1,61,0)] }),
    7: mkKey('OVDB', 0xffaa00, 6, { press: [cc(1,60,0)] }),
    8: mkKey('TAP',  0xff0000, 1, { ledmode: 'tap', press: [cc(1,64,64)] }),
    9: mkKey('UNDO', 0xffff00, 4, { press: [cc(1,63,127)] }),
  }, { encoder_name: 'VOL', exp1_cc: 1, exp2_cc: 11 });

  const fl = mkPage('FLSO', 'FL Studio', {
    0: mkKey('PLAY', 0x00ff00, 2, { press: [cc(1,102,127)] }),
    1: mkKey('STOP', 0xff0000, 1, { press: [cc(1,103,127)] }),
    2: mkKey('REC',  0xff0000, 1, { keytimes: 2, press: [cc(1,104,127)], release: [cc(1,104,0)] }),
    3: mkKey('MTRO', 0xffaa00, 6, { keytimes: 2, press: [cc(1,105,127)], release: [cc(1,105,0)] }),
    4: mkKey('PAT+', 0xffffff, 0, { press: [cc(1,106,127)] }),
    5: mkKey('SAVE', 0x0080ff, 3, { press: [cc(1,107,127)] }),
    6: mkKey('UNDO', 0xffff00, 4, { press: [cc(1,108,127)] }),
    7: mkKey('REDO', 0xaaff00, 4, { press: [cc(1,109,127)] }),
    8: mkKey('TAP',  0xff0000, 1, { ledmode: 'tap', press: [cc(1,64,64)] }),
    9: mkKey('PAT-', 0xffffff, 0, { press: [cc(1,110,127)] }),
  }, { encoder_name: 'VOL', exp1_cc: 11, exp2_cc: 7 });

  const reaper = mkPage('RPER', 'REAPER', {
    0: mkKey('PLAY', 0x00ff00, 2, { press: [cc(1,111,127)] }),
    1: mkKey('STOP', 0xff0000, 1, { press: [cc(1,112,127)] }),
    2: mkKey('REC',  0xff0000, 1, { keytimes: 2, press: [cc(1,113,127)], release: [cc(1,113,0)] }),
    3: mkKey('LOOP', 0x00ffff, 8, { keytimes: 2, press: [cc(1,114,127)], release: [cc(1,114,0)] }),
    4: mkKey('MRK+', 0xffffff, 0, { press: [cc(1,115,127)] }),
    5: mkKey('SAVE', 0x0080ff, 3, { press: [cc(1,116,127)] }),
    6: mkKey('UNDO', 0xffff00, 4, { press: [cc(1,117,127)] }),
    7: mkKey('REDO', 0xaaff00, 4, { press: [cc(1,118,127)] }),
    8: mkKey('TAP',  0xff0000, 1, { ledmode: 'tap', press: [cc(1,64,64)] }),
    9: mkKey('MRK-', 0xffffff, 0, { press: [cc(1,119,127)] }),
  }, { encoder_name: 'VOL', exp1_cc: 11, exp2_cc: 7 });

  return {
    mode: 'super',
    deviceFamily: 'std',
    activePage: 0,
    activeKey: 0,
    activeState: 0,
    pages: [katana, ndsp, helix, fl, reaper],
  };
}

function deviceSpec() {
  return DEVICE_FAMILIES[state.deviceFamily] || DEVICE_FAMILIES.std;
}
function deviceKeyCount() { return deviceSpec().keys; }

function applyLanguage(lang) {
  const t = I18N[lang] || I18N.en;
  document.querySelectorAll('[data-i18n]').forEach(el => {
    const key = el.dataset.i18n;
    if (t[key] == null) return;
    // Defensive: setting textContent on a parent of element children (e.g. a
    // <label> wrapping an <input>) destroys those children. Translate only
    // leaf elements. If you want to translate inside such a parent, wrap the
    // text in a <span data-i18n="..."> instead.
    if (el.children.length === 0) el.textContent = t[key];
  });
  state.language = lang;
  document.documentElement.setAttribute('lang', lang);
  const sel = document.querySelector('#lang-select');
  if (sel) sel.value = lang;
}

function tr(key) {
  const t = I18N[state.language || 'en'] || I18N.en;
  return t[key] || I18N.en[key] || key;
}

// ---------- 3. persistence + undo/redo ----------

let state = loadState() || defaultState();

const UNDO_CAP = 60;
const undoStack = [];
const redoStack = [];
let _lastSnap = JSON.stringify(state);
let _historyTimer = null;

function saveState() {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state)); } catch {}
  clearTimeout(_historyTimer);
  _historyTimer = setTimeout(maybePushHistory, 400);
}

function maybePushHistory() {
  const snap = JSON.stringify(state);
  if (snap === _lastSnap) return;
  undoStack.push(_lastSnap);
  if (undoStack.length > UNDO_CAP) undoStack.shift();
  redoStack.length = 0;
  _lastSnap = snap;
  refreshHistoryButtons();
}

function undo() {
  clearTimeout(_historyTimer);
  // flush any pending change so we don't lose it
  const pending = JSON.stringify(state);
  if (pending !== _lastSnap) {
    undoStack.push(_lastSnap);
    _lastSnap = pending;
  }
  if (!undoStack.length) return;
  const prev = undoStack.pop();
  redoStack.push(_lastSnap);
  _lastSnap = prev;
  state = JSON.parse(prev);
  try { localStorage.setItem(STORAGE_KEY, prev); } catch {}
  refreshHistoryButtons();
  render();
}

function redo() {
  clearTimeout(_historyTimer);
  if (!redoStack.length) return;
  const next = redoStack.pop();
  undoStack.push(_lastSnap);
  _lastSnap = next;
  state = JSON.parse(next);
  try { localStorage.setItem(STORAGE_KEY, next); } catch {}
  refreshHistoryButtons();
  render();
}

function refreshHistoryButtons() {
  const u = document.getElementById('undo-btn');
  const r = document.getElementById('redo-btn');
  if (u) u.disabled = undoStack.length === 0;
  if (r) r.disabled = redoStack.length === 0;
}

function loadState() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    // Migrate every key to the latest shape so older saved states still work.
    for (const page of parsed.pages || []) {
      for (const k of Object.values(page.keys || {})) migrateKey(k);
    }
    return parsed;
  } catch { return null; }
}

// ---------- 4. serializers ----------

function actionToSuper(a) {
  if (!a) return null;
  if (a._bank === 'inc') return `[1][PC][auto][bank_inc]`;
  if (a._bank === 'dec') return `[1][PC][auto][bank_dec]`;
  switch (a.type) {
    case 'CC': return `[${a.ch}][CC][${a.p1}][${a.p2}]`;
    case 'PC': return `[${a.ch}][PC][auto][${a.p1}]`;
    case 'NT': return `[${a.ch}][NT][${a.p1}][${a.p2}]`;
    case 'UP': return `[1][PC][auto][bank_inc]`;
    case 'DW': return `[1][PC][auto][bank_dec]`;
    default:   return null;
  }
}

function dimColor(rgb) {
  const r = ((rgb >> 16) & 0xff) >> 3;
  const g = ((rgb >> 8) & 0xff) >> 3;
  const b = (rgb & 0xff) >> 3;
  return (r << 16) | (g << 8) | b;
}

function emitSuperPage(page, idx) {
  const ledbright = 30, screenbright = 80;
  const lines = [
    '[globalsetup]',
    `ledbright = [${ledbright}]`,
    `screenbright = [${screenbright}]`,
    'dark_fonts = [off]',
    'wallpaper = [wp1]',
    'long_press_timing = [1]',
    'WIRELESS_2.4G = [on]',
    'WIRELESS_ID   = [8]',
    'WIRELESS_dB   = [6]',
    '',
    '',
    '[PAGE]',
    `page_name = [${page.name}]`,
    '',
    `exp1_CH = [${page.exp1_ch}]`,
    `exp1_CC = [${page.exp1_cc}]`,
    '',
    `exp2_CH = [${page.exp2_ch}]`,
    `exp2_CC = [${page.exp2_cc}]`,
    '',
    `encoder_CC = [${page.encoder_cc}]`,
    `encoder_NAME = [${page.encoder_name}]`,
    '',
    `midithrough = [${page.midithrough ? 'on' : 'off'}]`,
    '',
    `display_number_ABC = [${page.display_abc}]`,
    `group_number = [${page.group_number}]`,
    `display_pc_offset = [${page.pc_offset}]`,
    `display_bank_offset = [${page.bank_offset}]`,
    '',
  ];

  const keyCount = deviceKeyCount();
  for (let i = 0; i < keyCount; i++) {
    const k = migrateKey(page.keys[i] || mkKey('', 0x222222, 0));
    lines.push(`[key${i}]`);
    lines.push(`keytimes = [${k.keytimes}]`);
    lines.push(`ledmode = [${k.ledmode}]`);
    // Emit one block per tap state: ledcolor<N>, short_dw<N>, short_up<N>, long<N>, long_up<N>.
    for (let s = 0; s < k.keytimes; s++) {
      const stateColor = k.colors[s] != null
        ? k.colors[s]
        : (s === 0 ? k.color : dimColor(k.colors[0] != null ? k.colors[0] : k.color));
      const hex = `[0x${stateColor.toString(16).padStart(6, '0')}]`;
      if (s > 0) lines.push('');
      lines.push(`ledcolor${s + 1} = ${hex}${hex}${hex}`);
      if (k.press[s])         lines.push(`short_dw${s + 1} = ${actionToSuper(k.press[s])}`);
      if (k.release[s])       lines.push(`short_up${s + 1} = ${actionToSuper(k.release[s])}`);
      if (k.long[s])          lines.push(`long${s + 1} = ${actionToSuper(k.long[s])}`);
      if (k.long_release[s])  lines.push(`long_up${s + 1} = ${actionToSuper(k.long_release[s])}`);
    }
    lines.push('');
  }
  return lines.join('\n');
}

function slotLines(a) {
  if (!a) return ['1', '--', '0', '0', '-'];
  if (a._bank === 'inc') return ['1', 'UP', '0', '0', '-'];
  if (a._bank === 'dec') return ['1', 'DW', '0', '0', '-'];
  switch (a.type) {
    case 'CC': return [`${a.ch}`, 'CC', `${a.p1}`, `${a.p2}`, '-'];
    case 'PC': return [`${a.ch}`, 'PC', `${a.p1}`, '0', '-'];
    case 'NT': return [`${a.ch}`, 'NT', `${a.p1}`, `${a.p2}`, '-'];
    case 'UP': return ['1', 'UP', '0', '0', '-'];
    case 'DW': return ['1', 'DW', '0', '0', '-'];
    default:   return ['1', '--', '0', '0', '-'];
  }
}

function emitGeekKey(key) {
  const actions = [];
  for (let i = 0; i < 6; i++) actions.push(key.press[i] || null);
  for (let i = 0; i < 6; i++) actions.push(key.release[i] || null);
  const lines = [];
  for (const a of actions) lines.push(...slotLines(a));
  return lines.join('\r\n') + '\r\n';
}

function emitGeekLed(page) {
  const lines = [];
  for (let i = 0; i < 10; i++) {
    const k = page.keys[i] || mkKey('', 0, 0);
    lines.push(String(k.palette));
  }
  return lines.join('\r\n') + '\r\n';
}

function emitGeekSetup() {
  return [
    'SCREEN_LIGHT  = [80] # 1-100 Background brightness',
    'LED_LIGHT     = [30] # 0-100',
    'BATTERY_CHARGE= [OFF]',
    'WIRELESS_2.4G = [ON]',
    'WIRELESS_ID   = [8]',
    'MIDI_THROUGH  = [ON]',
    'EXP1_CC#      = [1]',
    'EXP2_CC#      = [7]',
    'WHEEL_MANUAL  = [ON]',
    'WIRELESS_dB   = [6]',
    '',
    '! Do not change anything other than inside the []',
    '',
  ].join('\r\n');
}

// ---------- 4b. parsers (import existing OEM / generated files) ----------

// SuperMode: parse a `pageN.txt` INI-ish file -> Page object.
function parseSuperPage(text) {
  const page = mkPage('XXXX', '', {});
  let section = null;
  let keyIdx = -1;
  const sectionRe = /^\[(\w+)\]/;
  const kvRe = /^(\w+)\s*=\s*(.+)$/;
  const stripBrackets = s => s.replace(/^\[(.*)\]$/, '$1');

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#') || line.startsWith('!')) continue;

    const sm = line.match(sectionRe);
    if (sm) {
      section = sm[1].toLowerCase();
      const km = section.match(/^key(\d+)$/);
      keyIdx = km ? +km[1] : -1;
      if (keyIdx >= 0 && !page.keys[keyIdx]) {
        page.keys[keyIdx] = mkKey('', 0x222222, 0);
      }
      continue;
    }

    const m = line.match(kvRe);
    if (!m) continue;
    const key = m[1];
    const val = m[2].trim();

    if (section === 'page') {
      switch (key) {
        case 'page_name': page.name = stripBrackets(val); break;
        case 'exp1_CH': page.exp1_ch = +stripBrackets(val); break;
        case 'exp1_CC': page.exp1_cc = +stripBrackets(val); break;
        case 'exp2_CH': page.exp2_ch = +stripBrackets(val); break;
        case 'exp2_CC': page.exp2_cc = +stripBrackets(val); break;
        case 'encoder_CC': page.encoder_cc = +stripBrackets(val); break;
        case 'encoder_NAME': page.encoder_name = stripBrackets(val); break;
        case 'midithrough': page.midithrough = stripBrackets(val).toLowerCase() === 'on'; break;
        case 'display_number_ABC': page.display_abc = stripBrackets(val); break;
        case 'group_number': page.group_number = +stripBrackets(val); break;
        case 'display_pc_offset': page.pc_offset = +stripBrackets(val); break;
        case 'display_bank_offset': page.bank_offset = +stripBrackets(val); break;
      }
    } else if (keyIdx >= 0) {
      const k = page.keys[keyIdx];
      let m2;
      if (key === 'keytimes') k.keytimes = +stripBrackets(val);
      else if (key === 'ledmode') k.ledmode = stripBrackets(val);
      else if ((m2 = key.match(/^ledcolor(\d+)$/))) {
        const idx = +m2[1] - 1;
        const colorMatch = val.match(/\[0x([0-9a-fA-F]{6})\]/);
        if (colorMatch) {
          const c = parseInt(colorMatch[1], 16);
          k.colors[idx] = c;
          if (idx === 0) k.color = c;
        }
      } else if ((m2 = key.match(/^short_dw(\d+)$/))) {
        const a = parseSuperAction(val);
        if (a) k.press[+m2[1] - 1] = a;
      } else if ((m2 = key.match(/^short_up(\d+)$/))) {
        const a = parseSuperAction(val);
        if (a) k.release[+m2[1] - 1] = a;
      } else if ((m2 = key.match(/^long_up(\d+)$/))) {
        const a = parseSuperAction(val);
        if (a) k.long_release[+m2[1] - 1] = a;
      } else if ((m2 = key.match(/^long(\d+)$/))) {
        const a = parseSuperAction(val);
        if (a) k.long[+m2[1] - 1] = a;
      }
    }
  }
  return page;
}

function parseSuperAction(val) {
  // val: "[1][CC][16][127]" or "[1][PC][auto][bank_inc]"
  const m = val.match(/\[([^\]]*)\]\[([^\]]*)\]\[([^\]]*)\]\[([^\]]*)\]/);
  if (!m) return null;
  const ch = +m[1] || 1;
  const ty = m[2].toUpperCase();
  const p1 = m[3];
  const p2 = m[4];
  if (ty === 'CC') return { type: 'CC', ch, p1: +p1, p2: +p2 };
  if (ty === 'NT') return { type: 'NT', ch, p1: +p1, p2: +p2 };
  if (ty === 'CCT') return { type: 'CC', ch, p1: +p1, p2: +p2 };  // treat CCT as CC
  if (ty === 'PC') {
    if (p2 === 'bank_inc') return bankInc();
    if (p2 === 'bank_dec') return bankDec();
    const num = +p2;
    if (Number.isFinite(num)) return { type: 'PC', ch, p1: num, p2: 0 };
    return null;
  }
  return null;
}

// GeekMode: parse one `gekey<N>.dat` -> Key object.
function parseGeekKey(text, paletteIdx = 0) {
  const lines = text.split(/\r?\n/).filter(l => l.length > 0 || true);
  // Records are groups of 5: ch, type, p1, p2, '-'
  const press = [];
  const release = [];
  for (let r = 0; r < 12; r++) {
    const base = r * 5;
    if (base + 3 >= lines.length) break;
    const ch = +lines[base] || 1;
    const ty = (lines[base + 1] || '').trim();
    const p1 = +lines[base + 2] || 0;
    const p2 = +lines[base + 3] || 0;
    let a = null;
    if (ty === 'CC') a = { type: 'CC', ch, p1, p2 };
    else if (ty === 'NT') a = { type: 'NT', ch, p1, p2 };
    else if (ty === 'PC') a = { type: 'PC', ch, p1, p2: 0 };
    else if (ty === 'UP') a = bankInc();
    else if (ty === 'DW') a = bankDec();
    if (a) {
      if (r < 6) press.push(a);
      else release.push(a);
    }
  }
  return mkKey('', 0xffffff, paletteIdx, { press, release });
}

// GeekMode: parse `keyled.dat` -> array of 10 palette indices.
function parseGeekLed(text) {
  return text.split(/\r?\n/).filter(s => s !== '').slice(0, 10).map(s => +s || 0);
}

// Orchestrator: take an array of {name, text} -> array of Page objects + new mode hint.
function importFiles(files) {
  // Categorize
  const superFiles = []; const geekKeyFiles = []; let geekLed = null;
  for (const f of files) {
    const n = f.name.toLowerCase();
    if (/^page\d+\.txt$/.test(n)) superFiles.push(f);
    else if (/^gekey\d+\.dat$/.test(n)) geekKeyFiles.push(f);
    else if (n === 'keyled.dat') geekLed = f;
    else if (n.endsWith('.txt')) superFiles.push(f);   // fallback
  }

  const result = { pages: [], mode: null, errors: [] };

  if (superFiles.length) {
    result.mode = 'super';
    superFiles.sort((a, b) => a.name.localeCompare(b.name));
    for (const f of superFiles) {
      try {
        const p = parseSuperPage(f.text);
        p.target = p.target || `(imported ${f.name})`;
        result.pages.push(p);
      } catch (err) {
        result.errors.push(`${f.name}: ${err.message}`);
      }
    }
  }

  if (geekKeyFiles.length) {
    if (!result.mode) result.mode = 'geek';
    // group by parent folder when path contains a slash; else single page
    const parentFolder = p => p.includes('/') ? p.replace(/\/[^\/]+$/, '') : '__one__';
    const groups = new Map();
    for (const f of geekKeyFiles) {
      const folder = parentFolder(f.path || f.name);
      if (!groups.has(folder)) groups.set(folder, { keys: [], led: null });
      groups.get(folder).keys.push(f);
    }
    if (geekLed) {
      const folder = parentFolder(geekLed.path || geekLed.name);
      if (groups.has(folder)) groups.get(folder).led = geekLed;
      else groups.set(folder, { keys: [], led: geekLed });
    }
    for (const [folder, grp] of groups) {
      const page = mkPage('IMPT', `(imported ${folder.replace('__one__', 'geek page')})`, {});
      const palettes = grp.led ? parseGeekLed(grp.led.text) : [];
      for (const f of grp.keys) {
        const idxMatch = f.name.match(/gekey(\d+)\.dat/i);
        if (!idxMatch) continue;
        const idx = +idxMatch[1];
        page.keys[idx] = parseGeekKey(f.text, palettes[idx] || 0);
      }
      result.pages.push(page);
    }
  }

  return result;
}

// ---------- 5. minimal STORED .zip writer (no compression) ----------
// Spec ref: https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT

function crc32(bytes) {
  let c, table = crc32._t;
  if (!table) {
    table = crc32._t = new Uint32Array(256);
    for (let i = 0; i < 256; i++) {
      c = i;
      for (let k = 0; k < 8; k++) c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
      table[i] = c >>> 0;
    }
  }
  c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = table[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function dosTimeDate(d = new Date()) {
  const t = ((d.getHours() & 0x1f) << 11) | ((d.getMinutes() & 0x3f) << 5) | ((d.getSeconds() / 2) & 0x1f);
  const dt = (((d.getFullYear() - 1980) & 0x7f) << 9) | (((d.getMonth() + 1) & 0xf) << 5) | (d.getDate() & 0x1f);
  return { time: t, date: dt };
}

function buildZip(files) {
  // files: [{ name: 'path/in/zip', bytes: Uint8Array }]
  const enc = new TextEncoder();
  const chunks = [];
  const central = [];
  let offset = 0;
  const { time, date } = dosTimeDate();

  for (const f of files) {
    const nameBytes = enc.encode(f.name);
    const data = f.bytes;
    const crc = crc32(data);
    const lh = new Uint8Array(30 + nameBytes.length);
    const dv = new DataView(lh.buffer);
    dv.setUint32(0, 0x04034b50, true);          // local file header sig
    dv.setUint16(4, 20, true);                  // version needed
    dv.setUint16(6, 0, true);                   // flags
    dv.setUint16(8, 0, true);                   // method = stored
    dv.setUint16(10, time, true);
    dv.setUint16(12, date, true);
    dv.setUint32(14, crc, true);
    dv.setUint32(18, data.length, true);
    dv.setUint32(22, data.length, true);
    dv.setUint16(26, nameBytes.length, true);
    dv.setUint16(28, 0, true);
    lh.set(nameBytes, 30);
    chunks.push(lh, data);

    const ch = new Uint8Array(46 + nameBytes.length);
    const cv = new DataView(ch.buffer);
    cv.setUint32(0, 0x02014b50, true);          // central dir sig
    cv.setUint16(4, 0x031e, true);              // version made by (unix, 30)
    cv.setUint16(6, 20, true);
    cv.setUint16(8, 0, true);
    cv.setUint16(10, 0, true);
    cv.setUint16(12, time, true);
    cv.setUint16(14, date, true);
    cv.setUint32(16, crc, true);
    cv.setUint32(20, data.length, true);
    cv.setUint32(24, data.length, true);
    cv.setUint16(28, nameBytes.length, true);
    cv.setUint16(30, 0, true);
    cv.setUint16(32, 0, true);
    cv.setUint16(34, 0, true);
    cv.setUint16(36, 0, true);
    cv.setUint32(38, 0, true);                  // ext attrs
    cv.setUint32(42, offset, true);
    ch.set(nameBytes, 46);
    central.push(ch);

    offset += lh.length + data.length;
  }

  const centralStart = offset;
  for (const c of central) {
    chunks.push(c);
    offset += c.length;
  }
  const centralSize = offset - centralStart;

  const eocd = new Uint8Array(22);
  const ev = new DataView(eocd.buffer);
  ev.setUint32(0, 0x06054b50, true);
  ev.setUint16(4, 0, true);
  ev.setUint16(6, 0, true);
  ev.setUint16(8, files.length, true);
  ev.setUint16(10, files.length, true);
  ev.setUint32(12, centralSize, true);
  ev.setUint32(16, centralStart, true);
  ev.setUint16(20, 0, true);
  chunks.push(eocd);

  let total = 0;
  for (const c of chunks) total += c.length;
  const out = new Uint8Array(total);
  let pos = 0;
  for (const c of chunks) { out.set(c, pos); pos += c.length; }
  return out;
}

// ---------- 5b. Web MIDI live preview ----------

let midiAccess = null;
let midiOutput = null;

async function initMIDI() {
  const status = document.querySelector('#midi-status');
  if (!navigator.requestMIDIAccess) {
    if (status) { status.textContent = 'Web MIDI not supported in this browser'; status.classList.add('is-err'); }
    return;
  }
  try {
    midiAccess = await navigator.requestMIDIAccess();
    refreshMidiOutputs();
    midiAccess.onstatechange = refreshMidiOutputs;
    // restore last-used port if still present
    if (state.midiOutputId) tryConnectMidi(state.midiOutputId);
  } catch (e) {
    if (status) { status.textContent = 'MIDI access denied'; status.classList.add('is-err'); }
  }
}

function refreshMidiOutputs() {
  const sel = document.querySelector('#midi-output');
  if (!sel || !midiAccess) return;
  const prev = sel.value;
  const outs = [...midiAccess.outputs.values()];
  sel.innerHTML = '<option value="">MIDI: —</option>' + outs.map(o => `<option value="${o.id}">${escapeAttr(o.name)}</option>`).join('');
  sel.value = (outs.find(o => o.id === prev) ? prev : '');
}

function escapeAttr(s) {
  return String(s).replace(/[&<>"']/g, c => ({ '&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;' }[c]));
}

function tryConnectMidi(id) {
  if (!midiAccess) return;
  midiOutput = id ? midiAccess.outputs.get(id) : null;
  const sel = document.querySelector('#midi-output');
  const status = document.querySelector('#midi-status');
  if (sel) {
    sel.value = id || '';
    sel.classList.toggle('is-connected', !!midiOutput);
  }
  if (status) {
    if (midiOutput) {
      status.textContent = `● connected to ${midiOutput.name}`;
      status.className = 'midi-status is-ok';
    } else {
      status.textContent = 'no MIDI output selected';
      status.className = 'midi-status';
    }
  }
  state.midiOutputId = id || '';
}

// Send one Action object as a MIDI message. SuperMode "bank_inc"/"bank_dec"
// are device-internal navigation and don't map to a wire message -- skip.
function sendMidiAction(a) {
  if (!midiOutput || !a) return;
  if (a._bank) return;
  const ch = ((a.ch || 1) - 1) & 0x0f;
  switch (a.type) {
    case 'CC':
      midiOutput.send([0xb0 | ch, (a.p1 & 0x7f), (a.p2 & 0x7f)]);
      break;
    case 'PC':
      midiOutput.send([0xc0 | ch, (a.p1 & 0x7f)]);
      break;
    case 'NT':
      midiOutput.send([0x90 | ch, (a.p1 & 0x7f), (a.p2 & 0x7f)]);
      // auto-release after 120 ms so connected gear doesn't hold notes
      setTimeout(() => midiOutput && midiOutput.send([0x80 | ch, (a.p1 & 0x7f), 0]), 120);
      break;
  }
}

function testActiveKey() {
  if (!midiOutput) {
    flashStatus('select a MIDI output first', 'is-err');
    return;
  }
  const k = migrateKey(curKey());
  let actions;
  if (state.mode === 'super') {
    const s = state.activeState;
    actions = [k.press[s], k.long[s], k.release[s], k.long_release[s]].filter(Boolean);
  } else {
    actions = [...k.press, ...k.release];
  }
  if (actions.length === 0) {
    flashStatus('no actions on this key', 'is-err');
    return;
  }
  // Stagger 60ms apart so each is observable separately
  actions.forEach((a, i) => setTimeout(() => sendMidiAction(a), i * 60));
  const btn = document.querySelector('#test-key-btn');
  btn?.classList.add('test-flash');
  setTimeout(() => btn?.classList.remove('test-flash'), 400);
  flashStatus(`sent ${actions.length} message${actions.length === 1 ? '' : 's'}`, 'is-ok');
}

let _flashTimer = null;
function flashStatus(msg, cls) {
  const status = document.querySelector('#midi-status');
  if (!status) return;
  status.textContent = msg;
  status.className = 'midi-status ' + (cls || '');
  clearTimeout(_flashTimer);
  _flashTimer = setTimeout(() => {
    if (midiOutput) {
      status.textContent = `● connected to ${midiOutput.name}`;
      status.className = 'midi-status is-ok';
    } else {
      status.textContent = 'no MIDI output selected';
      status.className = 'midi-status';
    }
  }, 1500);
}

// ---------- 6. rendering ----------

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => document.querySelectorAll(sel);

function render() {
  document.body.dataset.mode = state.mode;
  document.body.dataset.device = state.deviceFamily || 'std';
  applyLanguage(state.language || 'en');
  const sel = $('#device-select');
  if (sel) sel.value = state.deviceFamily || 'std';
  $$('.mode-btn').forEach(b => b.classList.toggle('is-active', b.dataset.mode === state.mode));
  $$('.mode-btn').forEach(b => b.setAttribute('aria-selected', b.dataset.mode === state.mode));

  renderPageList();
  renderPageMeta();
  renderBoard();
  renderKeyEditor();
  renderPreview();
}

function renderPageList() {
  const ol = $('#page-list');
  ol.innerHTML = '';
  state.pages.forEach((p, i) => {
    const li = document.createElement('li');
    li.classList.toggle('is-active', i === state.activePage);
    li.innerHTML = `
      <span class="page-idx">${state.mode === 'super' ? `P${i}` : `P${i + 1}`}</span>
      <span class="page-name-cell">${p.name}</span>
      <span class="page-tgt">${p.target || ''}</span>
    `;
    li.addEventListener('click', () => {
      state.activePage = i;
      state.activeKey = 0;
      saveState();
      render();
    });
    ol.appendChild(li);
  });
}

function curPage() { return state.pages[state.activePage]; }
function curKey() { return curPage().keys[state.activeKey] || mkKey('', 0x222222, 0); }
function ensureCurKey() {
  if (!curPage().keys[state.activeKey]) {
    curPage().keys[state.activeKey] = mkKey('', 0x222222, 0);
  }
  return curPage().keys[state.activeKey];
}

function renderPageMeta() {
  const p = curPage();
  $('#meta-name').value = p.name;
  $('#meta-enc-cc').value = p.encoder_cc;
  $('#meta-enc-name').value = p.encoder_name;
  $('#meta-thru').value = p.midithrough ? 'on' : 'off';
  $('#meta-exp1-ch').value = p.exp1_ch;
  $('#meta-exp1-cc').value = p.exp1_cc;
  $('#meta-exp2-ch').value = p.exp2_ch;
  $('#meta-exp2-cc').value = p.exp2_cc;
  $('#meta-display-abc').value = p.display_abc;
  $('#meta-group').value = p.group_number;
}

// SVG board geometry (in viewBox units; viewBox is 760x360)
// Switch positions depend on device family so we compute on demand.
function boardGeom() {
  const spec = deviceSpec();
  const r = 38, gap = 32;
  const totalW = spec.cols * (2 * r) + (spec.cols - 1) * gap;
  const totalH = spec.rows * (2 * r) + (spec.rows - 1) * gap;
  const startX = (760 - totalW) / 2 + r;
  const startY = spec.rows === 1 ? 230 : 200;
  const switches = [];
  for (let row = 0; row < spec.rows; row++) {
    for (let col = 0; col < spec.cols; col++) {
      switches.push({
        cx: startX + col * (2 * r + gap),
        cy: startY + row * (2 * r + gap),
        r,
      });
      if (switches.length >= spec.keys) break;
    }
    if (switches.length >= spec.keys) break;
  }
  return {
    chassis: { x: 8, y: 8, w: 744, h: 344, r: 4 },
    display: { cx: 380, cy: 70, w: 130, h: 80 },
    encoder: { cx: 560, cy: 70, r: 24 },
    switches,
  };
}

function ledColorHex(k) {
  return state.mode === 'super'
    ? '#' + k.color.toString(16).padStart(6, '0')
    : (PALETTE[k.palette]?.[0] || '#222');
}

function renderBoard() {
  const svg = $('#board-svg');
  const g = boardGeom();
  const page = curPage();
  const n = deviceKeyCount();

  // defs (gradient for switch tops)
  const defs = `
    <defs>
      <radialGradient id="switch-grad" cx="0.35" cy="0.35" r="0.7">
        <stop offset="0%" stop-color="#3c3c44"/>
        <stop offset="100%" stop-color="#1a1a1e"/>
      </radialGradient>
      <linearGradient id="display-glow" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#101014"/>
        <stop offset="100%" stop-color="#050507"/>
      </linearGradient>
    </defs>
  `;

  // chassis + brand plate + screws
  const screws = [[20,20],[740,20],[20,340],[740,340]]
    .map(([x,y]) => `<circle cx="${x}" cy="${y}" r="3.5" fill="#1a1a1e" stroke="#3a3a40"/>`)
    .join('');

  const chassis = `
    <rect class="chassis" x="${g.chassis.x}" y="${g.chassis.y}" width="${g.chassis.w}" height="${g.chassis.h}" rx="${g.chassis.r}"/>
    <rect class="chassis-detail" x="${g.chassis.x + 6}" y="${g.chassis.y + 6}" width="${g.chassis.w - 12}" height="${g.chassis.h - 12}"/>
    ${screws}
    <rect class="brand-plate" x="40" y="40" width="120" height="48"/>
    <text class="brand-text" x="100" y="56" text-anchor="middle">MIDI&#160;CAPTAIN</text>
    <text class="brand-text" x="100" y="68" text-anchor="middle">${deviceSpec().name.toUpperCase()}</text>
    <text class="brand-text" x="100" y="80" text-anchor="middle" style="fill:var(--accent);opacity:0.6">REMEDY</text>
  `;

  // display
  const d = g.display;
  const dx = d.cx - d.w/2, dy = d.cy - d.h/2;
  const display = `
    <rect class="display-frame" x="${dx-4}" y="${dy-4}" width="${d.w+8}" height="${d.h+8}"/>
    <rect class="display-glass" x="${dx}" y="${dy}" width="${d.w}" height="${d.h}" fill="url(#display-glow)"/>
    <text class="display-text display-sub" x="${d.cx}" y="${dy + 14}">${state.mode === 'super' ? 'P' + state.activePage : 'P' + (state.activePage + 1)}</text>
    <text class="display-text display-name" x="${d.cx}" y="${d.cy + 4}">${page.name || '----'}</text>
    <text class="display-text display-sub" x="${d.cx}" y="${dy + d.h - 10}">${page.target || ''}</text>
  `;

  // encoder
  const e = g.encoder;
  const encoder = `
    <circle class="encoder" cx="${e.cx}" cy="${e.cy}" r="${e.r}"/>
    <circle class="encoder-cap" cx="${e.cx}" cy="${e.cy}" r="${e.r - 6}"/>
    <line class="encoder-tick" x1="${e.cx}" y1="${e.cy - e.r + 9}" x2="${e.cx}" y2="${e.cy - e.r + 16}"/>
    <text class="encoder-label" x="${e.cx}" y="${e.cy + e.r + 12}">${page.encoder_name || 'ENC'}</text>
    <text class="encoder-label" x="${e.cx}" y="${e.cy + e.r + 21}">CC ${page.encoder_cc}</text>
  `;

  // switches
  let switches = '';
  for (let i = 0; i < n; i++) {
    const k = page.keys[i] || mkKey('', 0x222222, 0);
    const s = g.switches[i];
    const color = ledColorHex(k);
    const selected = i === state.activeKey ? 'is-selected' : '';
    const sub = describeKey(k);
    const label = k.label || '·';
    switches += `
      <g class="switch ${selected}" data-key="${i}" style="color:${color}">
        <ellipse class="switch-shadow" cx="${s.cx + 1}" cy="${s.cy + 3}" rx="${s.r}" ry="${s.r - 2}"/>
        <circle class="switch-halo" cx="${s.cx}" cy="${s.cy}" r="${s.r + 6}"/>
        <circle class="switch-led" cx="${s.cx}" cy="${s.cy}" r="${s.r}" stroke="${color}"/>
        <circle class="switch-body" cx="${s.cx}" cy="${s.cy}" r="${s.r - 4}"/>
        <circle class="switch-top" cx="${s.cx}" cy="${s.cy}" r="${s.r - 10}"/>
        <text class="switch-idx" x="${s.cx}" y="${s.cy - s.r - 4}">K${i}</text>
        <text class="switch-label" x="${s.cx}" y="${s.cy + 2}" fill="${color}" style="text-shadow:0 0 6px ${color}">${label}</text>
        <text class="switch-sub" x="${s.cx}" y="${s.cy + s.r + 12}">${sub}</text>
      </g>
    `;
  }

  svg.innerHTML = defs + chassis + display + encoder + switches;

  // wire switch clicks (event delegation on svg)
  svg.onclick = (e) => {
    const g = e.target.closest('.switch');
    if (!g) return;
    const i = +g.dataset.key;
    state.activeKey = i;
    saveState();
    render();
  };
}

function describeKey(k) {
  if (!k.press.length && !k.release.length) return '—';
  const a = k.press[0] || k.release[0];
  if (!a) return '—';
  if (a._bank === 'inc') return 'BANK+';
  if (a._bank === 'dec') return 'BANK-';
  if (a.type === 'CC') return `CC${a.p1}=${a.p2}`;
  if (a.type === 'PC') return `PC${a.p1}`;
  if (a.type === 'NT') return `NOTE${a.p1}`;
  if (a.type === 'UP') return 'PAGE+';
  if (a.type === 'DW') return 'PAGE-';
  return a.type;
}

function renderKeyEditor() {
  const k = migrateKey(curKey());
  // Clamp active tap state to the current keytimes
  if (state.activeState == null || state.activeState >= k.keytimes) state.activeState = 0;
  const s = state.activeState;
  const stateColor = k.colors[s] != null ? k.colors[s] : (s === 0 ? k.color : dimColor(k.colors[0] != null ? k.colors[0] : k.color));

  $('#key-edit-title').textContent = `KEY ${state.activeKey}`;
  $('#key-edit-hint').textContent = KEY_HINTS[state.activeKey] || '';
  $('#key-label').value = k.label;
  $('#key-color').value = '#' + stateColor.toString(16).padStart(6, '0');
  $('#key-palette').value = k.palette;
  $('#key-ledmode').value = k.ledmode;
  $('#key-keytimes').value = k.keytimes;
  $('#key-color-hint').textContent = state.mode === 'super' && k.keytimes > 1 ? `${tr('state_n').toLowerCase()} ${s + 1}` : '';

  renderStateTabs(k);
  if (state.mode === 'super') {
    renderActionList('press',        k.press[s]        ? [k.press[s]]        : []);
    renderActionList('release',      k.release[s]      ? [k.release[s]]      : []);
    renderActionList('long',         k.long[s]         ? [k.long[s]]         : []);
    renderActionList('long_release', k.long_release[s] ? [k.long_release[s]] : []);
  } else {
    // GeekMode: show the full action stack on state 1 (no multi-state)
    renderActionList('press',   k.press);
    renderActionList('release', k.release);
    renderActionList('long',         []);
    renderActionList('long_release', []);
  }
}

function renderStateTabs(k) {
  const wrap = $('#state-tabs');
  if (state.mode !== 'super' || k.keytimes <= 1) {
    wrap.hidden = true;
    return;
  }
  wrap.hidden = false;
  wrap.innerHTML = '';
  for (let i = 0; i < k.keytimes; i++) {
    const tab = document.createElement('button');
    tab.type = 'button';
    tab.className = 'state-tab' + (i === state.activeState ? ' is-active' : '');
    const stateColor = k.colors[i] != null ? k.colors[i] : (i === 0 ? k.color : dimColor(k.colors[0] != null ? k.colors[0] : k.color));
    const colorHex = '#' + stateColor.toString(16).padStart(6, '0');
    tab.innerHTML = `<span class="state-dot" style="background:${colorHex}"></span>${tr('state_n')} ${i + 1}`;
    tab.addEventListener('click', () => {
      state.activeState = i;
      saveState();
      renderKeyEditor();
    });
    wrap.appendChild(tab);
  }
}

function renderActionList(kind, actions) {
  const ol = $(`#${kind}-list`);
  ol.innerHTML = '';
  actions.forEach((a, idx) => {
    ol.appendChild(actionRow(kind, idx, a));
  });
}

function actionRow(kind, idx, a) {
  const li = document.createElement('li');
  li.className = 'action-row';

  const typeSel = document.createElement('select');
  for (const t of ACTION_TYPES) {
    const opt = document.createElement('option');
    opt.value = t;
    opt.textContent = t;
    opt.title = TYPE_DESCRIPTIONS[t];
    if (a.type === t && !a._bank) opt.selected = true;
    typeSel.appendChild(opt);
  }
  // Add a special "BANK" pseudo-type for super-bank actions
  const bankOpt = document.createElement('option');
  bankOpt.value = a._bank === 'dec' ? 'BANK-' : 'BANK+';
  bankOpt.textContent = a._bank === 'dec' ? 'BANK-' : 'BANK+';
  if (a._bank) { bankOpt.selected = true; typeSel.appendChild(bankOpt); }

  const chIn = document.createElement('input');
  chIn.type = 'number'; chIn.min = 1; chIn.max = 16; chIn.value = a.ch || 1;
  chIn.title = 'MIDI channel 1-16';

  const p1In = document.createElement('input');
  p1In.type = 'number'; p1In.min = 0; p1In.max = 127; p1In.value = a.p1 ?? 0;
  p1In.title = labelForP1(a);

  const p2In = document.createElement('input');
  p2In.type = 'number'; p2In.min = 0; p2In.max = 127; p2In.value = a.p2 ?? 0;
  p2In.title = labelForP2(a);

  const rm = document.createElement('button');
  rm.type = 'button'; rm.className = 'rm'; rm.textContent = '×';
  rm.title = 'Remove this action';

  typeSel.addEventListener('change', () => {
    updateAction(kind, idx, { newType: typeSel.value });
  });
  chIn.addEventListener('input', () => updateAction(kind, idx, { ch: +chIn.value || 1 }));
  p1In.addEventListener('input', () => updateAction(kind, idx, { p1: clamp(+p1In.value, 0, 127) }));
  p2In.addEventListener('input', () => updateAction(kind, idx, { p2: clamp(+p2In.value, 0, 127) }));
  rm.addEventListener('click', () => removeAction(kind, idx));

  li.appendChild(typeSel);
  li.appendChild(chIn);
  li.appendChild(p1In);
  li.appendChild(p2In);
  li.appendChild(rm);
  return li;
}

function labelForP1(a) {
  if (a.type === 'CC') return 'CC number 0-127';
  if (a.type === 'PC') return 'Program number 0-127';
  if (a.type === 'NT') return 'Note number 0-127';
  return 'param 1';
}
function labelForP2(a) {
  if (a.type === 'CC') return 'CC value 0-127';
  if (a.type === 'NT') return 'Velocity 0-127';
  return 'param 2';
}
function clamp(n, lo, hi) { return Math.max(lo, Math.min(hi, n)); }

function renderPreview() {
  const idx = state.activePage;
  const page = curPage();
  let title, content;
  if (state.mode === 'super') {
    title = `page${idx}.txt`;
    content = emitSuperPage(page, idx);
  } else {
    title = `page${idx + 1}/gekey${state.activeKey}.dat`;
    content = emitGeekKey(curKey());
  }
  $('#preview-title').textContent = `PREVIEW / ${title}`;
  $('#preview').textContent = content;
}

// ---------- 7. event wiring ----------

// Resolve where a displayed action lives in the data model.
// SuperMode: every kind is per-state (k[kind][activeState] holds the single action).
// GeekMode:  press/release are stacks (k[kind][idx]); long/long_release are super-only.
function actionLoc(kind, displayIdx) {
  const k = migrateKey(ensureCurKey());
  if (state.mode === 'super') return { k, arr: k[kind], idx: state.activeState };
  return { k, arr: k[kind], idx: displayIdx };
}

function updateAction(kind, displayIdx, change) {
  const loc = actionLoc(kind, displayIdx);
  let a = loc.arr[loc.idx];
  if (!a) return;
  if (change.newType !== undefined) {
    const t = change.newType;
    if (t === 'BANK+')      a = { type: 'PC', ch: 1, p1: -1, p2: 0, _bank: 'inc' };
    else if (t === 'BANK-') a = { type: 'PC', ch: 1, p1: -1, p2: 0, _bank: 'dec' };
    else { delete a._bank; a.type = t; }
  } else {
    Object.assign(a, change);
  }
  loc.arr[loc.idx] = a;
  saveState();
  renderBoard();
  renderKeyEditor();
  renderPreview();
}

function removeAction(kind, displayIdx) {
  const loc = actionLoc(kind, displayIdx);
  if (state.mode === 'super') {
    loc.arr[loc.idx] = undefined;
  } else {
    loc.arr.splice(loc.idx, 1);
  }
  saveState();
  renderBoard();
  renderKeyEditor();
  renderPreview();
}

function addAction(kind) {
  const k = migrateKey(ensureCurKey());
  const fresh = { type: 'CC', ch: 1, p1: 0, p2: 127 };
  if (state.mode === 'super') {
    // SuperMode allows one action per kind per state. Overwrite if present.
    k[kind][state.activeState] = fresh;
  } else {
    if (kind === 'long' || kind === 'long_release') return;  // super-only
    if (k[kind].length >= 6) return;
    k[kind].push(fresh);
  }
  saveState();
  renderKeyEditor();
  renderBoard();
  renderPreview();
}

function wireEvents() {
  // undo / redo (buttons + keyboard)
  $('#undo-btn').addEventListener('click', undo);
  $('#redo-btn').addEventListener('click', redo);

  // import
  $('#import-input').addEventListener('change', async (e) => {
    const fileList = Array.from(e.target.files);
    if (!fileList.length) return;
    const files = await Promise.all(fileList.map(async f => ({
      name: f.name,
      path: f.webkitRelativePath || f.name,
      text: await f.text(),
    })));
    const result = importFiles(files);
    if (result.errors.length) {
      showToast(`${result.errors.length} parse error(s)`);
    }
    if (!result.pages.length) {
      showToast('no pages found');
      e.target.value = '';
      return;
    }
    if (result.mode && result.mode !== state.mode) {
      state.mode = result.mode;
    }
    state.pages.push(...result.pages);
    state.activePage = state.pages.length - result.pages.length;
    state.activeKey = 0;
    saveState();
    render();
    showToast(`imported ${result.pages.length} page${result.pages.length === 1 ? '' : 's'}`);
    e.target.value = '';
  });
  document.addEventListener('keydown', e => {
    if (!(e.ctrlKey || e.metaKey)) return;
    const k = e.key.toLowerCase();
    if (k === 'z' && !e.shiftKey) { e.preventDefault(); undo(); }
    else if ((k === 'z' && e.shiftKey) || k === 'y') { e.preventDefault(); redo(); }
  });

  // mode switch
  $$('.mode-btn').forEach(b => b.addEventListener('click', () => {
    state.mode = b.dataset.mode;
    saveState();
    render();
  }));

  // device family
  $('#device-select').addEventListener('change', e => {
    state.deviceFamily = e.target.value;
    // clamp activeKey to new device's key count
    const n = deviceKeyCount();
    if (state.activeKey >= n) state.activeKey = 0;
    saveState();
    render();
  });

  // language
  $('#lang-select').addEventListener('change', e => {
    applyLanguage(e.target.value);
    saveState();
  });

  // MIDI output picker
  $('#midi-output').addEventListener('change', e => {
    tryConnectMidi(e.target.value);
    saveState();
  });
  $('#test-key-btn').addEventListener('click', testActiveKey);

  // page actions
  $('#page-add').addEventListener('click', () => {
    state.pages.push(mkPage('NEW', '', {
      4: mkKey('BANK+', 0xffffff, 0, { press: [bankInc()] }),
      9: mkKey('BANK-', 0xffffff, 0, { press: [bankDec()] }),
    }));
    state.activePage = state.pages.length - 1;
    state.activeKey = 0;
    saveState();
    render();
  });

  $('#page-dup').addEventListener('click', () => {
    const clone = JSON.parse(JSON.stringify(curPage()));
    state.pages.splice(state.activePage + 1, 0, clone);
    state.activePage++;
    saveState();
    render();
  });

  $('#page-del').addEventListener('click', () => {
    if (state.pages.length <= 1) return;
    if (!confirm(`Delete page ${state.activePage} (${curPage().name})?`)) return;
    state.pages.splice(state.activePage, 1);
    state.activePage = Math.max(0, state.activePage - 1);
    saveState();
    render();
  });

  // page meta
  const metaBindings = [
    ['#meta-name', v => curPage().name = v.toUpperCase().slice(0, 4)],
    ['#meta-enc-cc', v => curPage().encoder_cc = clamp(+v, 0, 127)],
    ['#meta-enc-name', v => curPage().encoder_name = v.toUpperCase().slice(0, 4)],
    ['#meta-thru', v => curPage().midithrough = v === 'on'],
    ['#meta-exp1-ch', v => curPage().exp1_ch = clamp(+v, 1, 16)],
    ['#meta-exp1-cc', v => curPage().exp1_cc = clamp(+v, 0, 127)],
    ['#meta-exp2-ch', v => curPage().exp2_ch = clamp(+v, 1, 16)],
    ['#meta-exp2-cc', v => curPage().exp2_cc = clamp(+v, 0, 127)],
    ['#meta-display-abc', v => curPage().display_abc = v],
    ['#meta-group', v => curPage().group_number = +v],
  ];
  for (const [sel, fn] of metaBindings) {
    $(sel).addEventListener('input', e => {
      fn(e.target.value);
      saveState();
      renderPageList();
      renderPreview();
    });
  }

  // key editor
  $('#key-label').addEventListener('input', e => {
    ensureCurKey().label = e.target.value.toUpperCase().slice(0, 4);
    saveState(); renderBoard(); renderPreview();
  });
  $('#key-color').addEventListener('input', e => {
    const k = migrateKey(ensureCurKey());
    const c = parseInt(e.target.value.slice(1), 16);
    k.colors[state.activeState] = c;
    if (state.activeState === 0) k.color = c;     // keep legacy field in sync for state-1
    saveState(); renderBoard(); renderKeyEditor(); renderPreview();
  });
  $('#key-palette').addEventListener('input', e => {
    ensureCurKey().palette = clamp(+e.target.value, 0, 21);
    saveState(); renderBoard(); renderPreview();
  });
  $('#key-ledmode').addEventListener('change', e => {
    ensureCurKey().ledmode = e.target.value;
    saveState(); renderPreview();
  });
  $('#key-keytimes').addEventListener('change', e => {
    const k = migrateKey(ensureCurKey());
    k.keytimes = clamp(+e.target.value, 1, 10);
    // Top up colors[] with dim-of-state-0 for any newly added states
    while (k.colors.length < k.keytimes) {
      const base = k.colors[0] != null ? k.colors[0] : k.color;
      k.colors.push(dimColor(base));
    }
    saveState(); renderKeyEditor(); renderBoard(); renderPreview();
  });

  $$('[data-add-action]').forEach(b => b.addEventListener('click', () => {
    addAction(b.dataset.addAction);
  }));

  // copy & download
  $('#copy-btn').addEventListener('click', async () => {
    const text = $('#preview').textContent;
    try {
      await navigator.clipboard.writeText(text);
      showToast('copied');
    } catch {
      showToast('copy failed');
    }
  });

  $('#dl-btn').addEventListener('click', () => {
    const text = $('#preview').textContent;
    const idx = state.activePage;
    const filename = state.mode === 'super'
      ? `page${idx}.txt`
      : `gekey${state.activeKey}.dat`;
    downloadBytes(new TextEncoder().encode(text), filename);
  });

  $('#dl-all-btn').addEventListener('click', () => {
    const files = buildAllFiles();
    const zip = buildZip(files);
    const name = state.mode === 'super' ? 'supersetup.zip' : 'geeksetup.zip';
    downloadBytes(zip, name);
  });
}

function buildAllFiles() {
  const out = [];
  const enc = new TextEncoder();
  if (state.mode === 'super') {
    state.pages.forEach((p, i) => {
      out.push({ name: `page${i}.txt`, bytes: enc.encode(emitSuperPage(p, i)) });
    });
  } else {
    out.push({ name: 'GeekSetup.txt', bytes: enc.encode(emitGeekSetup()) });
    const keyCount = deviceKeyCount();
    state.pages.forEach((p, idx) => {
      const pn = `page${idx + 1}/`;
      for (let i = 0; i < keyCount; i++) {
        const k = p.keys[i] || mkKey('', 0, 0);
        out.push({ name: `${pn}gekey${i}.dat`, bytes: enc.encode(emitGeekKey(k)) });
      }
      out.push({ name: `${pn}keyled.dat`, bytes: enc.encode(emitGeekLed(p)) });
    });
  }
  return out;
}

function downloadBytes(bytes, filename) {
  const blob = new Blob([bytes], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  setTimeout(() => {
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }, 0);
}

let toastTimer = null;
function showToast(msg) {
  const t = $('#copy-toast');
  t.textContent = msg;
  t.classList.add('is-visible');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('is-visible'), 1200);
}

// ---------- 8. init ----------

wireEvents();
render();
initMIDI();

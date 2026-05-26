"""
MIDI Captain preset generator -- both SuperMode and GeekMode.

USAGE
-----
    python generate.py --mode super --out ./presets/supersetup
    python generate.py --mode geek  --out ./presets/geeksetup

EDITING
-------
Edit the PAGES dict near the bottom of this file. Each page is built from
a dict of `{key_index: Key(...)}` plus page-level metadata.

A Key has:
    label  -- 4-char display label (SuperMode only)
    press  -- list of actions sent on press (max 6 in GeekMode, 1-3 cycles in SuperMode)
    release-- list of actions sent on release (max 6 in GeekMode, optional in SuperMode)
    long   -- long-press action (SuperMode only)
    color  -- RGB hex 0xRRGGBB (SuperMode) or palette index 0-21 (GeekMode)
    keytimes -- SuperMode multi-tap state count (1, 2, or 3)
    ledmode  -- SuperMode 'normal' | 'select' | 'tap'

An Action is one of:
    cc(ch, num, val)
    pc(ch, num)
    nt(ch, num, vel)
    bank_inc() / bank_dec()       -- SuperMode PC nav (renders as DW-side UP/DOWN
                                     in GeekMode by convention on keys 4/9)
    up() / dw()                   -- GeekMode page nav
"""

from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


# --- Action types ------------------------------------------------------------

@dataclass
class Action:
    type: str         # 'CC' | 'PC' | 'NT' | 'UP' | 'DW' | 'BANK_INC' | 'BANK_DEC'
    ch: int = 1
    p1: int = 0
    p2: int = 0


def cc(ch, num, val): return Action('CC', ch, num, val)
def pc(ch, num):       return Action('PC', ch, num, 0)
def nt(ch, num, vel):  return Action('NT', ch, num, vel)
def up():              return Action('UP')
def dw():              return Action('DW')
def bank_inc():        return Action('BANK_INC')
def bank_dec():        return Action('BANK_DEC')


# --- Key + Page model --------------------------------------------------------

@dataclass
class Key:
    label: str = ''
    press: list = field(default_factory=list)
    release: list = field(default_factory=list)
    long: Optional[Action] = None
    color: int = 0xffffff           # RGB or palette idx depending on emit mode
    palette: int = 0                # GeekMode palette idx if color is RGB
    keytimes: int = 1
    ledmode: str = 'normal'


@dataclass
class Page:
    name: str                        # 4-char page label
    keys: dict                       # {0..9: Key}
    encoder_cc: int = 7
    encoder_name: str = 'VOL'
    exp1_ch: int = 1
    exp1_cc: int = 1
    exp2_ch: int = 1
    exp2_cc: int = 7
    midithrough: bool = True
    display_abc: str = 'abc4'
    group_number: int = 4
    pc_offset: int = 1
    bank_offset: int = 1


# --- SuperMode emitter -------------------------------------------------------

GLOBAL_SUPER = """\
[globalsetup]
ledbright = [30]
screenbright = [80]
dark_fonts = [off]
wallpaper = [wp1]
long_press_timing = [1]
WIRELESS_2.4G = [on]
WIRELESS_ID   = [8]
WIRELESS_dB   = [6]
"""


def _action_super(a: Action) -> str:
    if a.type == 'CC':
        return f'[{a.ch}][CC][{a.p1}][{a.p2}]'
    if a.type == 'PC':
        return f'[{a.ch}][PC][auto][{a.p1}]'
    if a.type == 'NT':
        return f'[{a.ch}][NT][{a.p1}][{a.p2}]'
    if a.type == 'BANK_INC':
        return f'[1][PC][auto][bank_inc]'
    if a.type == 'BANK_DEC':
        return f'[1][PC][auto][bank_dec]'
    if a.type in ('UP', 'DW'):
        # SuperMode has no UP/DW -- these belong to GeekMode.
        # Map to bank nav as the SuperMode equivalent.
        return f'[1][PC][auto][bank_{"inc" if a.type == "UP" else "dec"}]'
    raise ValueError(f'unknown action type {a.type}')


def emit_super_page(p: Page, is_page0: bool) -> str:
    parts = []
    parts.append(GLOBAL_SUPER if is_page0 else GLOBAL_SUPER)  # always include for portability
    parts.append('\n[PAGE]\n')
    parts.append(f'page_name = [{p.name}]\n\n')
    parts.append(f'exp1_CH = [{p.exp1_ch}]\nexp1_CC = [{p.exp1_cc}]\n\n')
    parts.append(f'exp2_CH = [{p.exp2_ch}]\nexp2_CC = [{p.exp2_cc}]\n\n')
    parts.append(f'encoder_CC = [{p.encoder_cc}]\nencoder_NAME = [{p.encoder_name}]\n\n')
    parts.append(f'midithrough = [{"on" if p.midithrough else "off"}]\n\n')
    parts.append(f'display_number_ABC = [{p.display_abc}]\n')
    parts.append(f'group_number = [{p.group_number}]\n')
    parts.append(f'display_pc_offset = [{p.pc_offset}]\n')
    parts.append(f'display_bank_offset = [{p.bank_offset}]\n\n')

    for i in range(10):
        k = p.keys.get(i, Key(color=0x222222))
        parts.append(f'[key{i}]\n')
        parts.append(f'keytimes = [{k.keytimes}]\n')
        parts.append(f'ledmode = [{k.ledmode}]\n')
        # state 1
        rgb = f'[0x{k.color:06x}]'
        parts.append(f'ledcolor1 = {rgb}{rgb}{rgb}\n')
        if k.press:
            parts.append(f'short_dw1 = {_action_super(k.press[0])}\n')
        if k.release:
            parts.append(f'short_up1 = {_action_super(k.release[0])}\n')
        if k.long:
            parts.append(f'long1 = {_action_super(k.long)}\n')
        # state 2 (use dim color of press0 / second press action if any)
        if k.keytimes >= 2:
            r = ((k.color >> 16) & 0xff) >> 3
            g = ((k.color >> 8) & 0xff) >> 3
            b = (k.color & 0xff) >> 3
            dim = (r << 16) | (g << 8) | b
            drgb = f'[0x{dim:06x}]'
            parts.append(f'\nledcolor2 = {drgb}{drgb}{drgb}\n')
            second = k.press[1] if len(k.press) > 1 else None
            if second:
                parts.append(f'short_dw2 = {_action_super(second)}\n')
        parts.append('\n')
    return ''.join(parts)


def emit_super(pages: list, out_dir: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    for idx, page in enumerate(pages):
        (out_dir / f'page{idx}.txt').write_text(emit_super_page(page, idx == 0))


# --- GeekMode emitter --------------------------------------------------------

GEEK_SETUP_TXT = """\
SCREEN_LIGHT  = [80] # 1-100 Background brightness
LED_LIGHT     = [30] # 0-100
BATTERY_CHARGE= [OFF]
WIRELESS_2.4G = [ON]
WIRELESS_ID   = [8]
MIDI_THROUGH  = [ON]
EXP1_CC#      = [1]
EXP2_CC#      = [7]
WHEEL_MANUAL  = [ON]
WIRELESS_dB   = [6]

! Do not change anything other than inside the []
"""


def _slot_lines(a: Optional[Action]) -> list:
    if a is None:
        return ['1', '--', '0', '0', '-']
    if a.type == 'CC':
        return [str(a.ch), 'CC', str(a.p1), str(a.p2), '-']
    if a.type == 'PC':
        return [str(a.ch), 'PC', str(a.p1), '0', '-']
    if a.type == 'NT':
        return [str(a.ch), 'NT', str(a.p1), str(a.p2), '-']
    if a.type in ('UP', 'BANK_INC'):
        return ['1', 'UP', '0', '0', '-']
    if a.type in ('DW', 'BANK_DEC'):
        return ['1', 'DW', '0', '0', '-']
    raise ValueError(f'unknown action type {a.type}')


def emit_geek_key(k: Key) -> bytes:
    actions = (k.press + [None] * 6)[:6] + (k.release + [None] * 6)[:6]
    lines = []
    for a in actions:
        lines.extend(_slot_lines(a))
    return ('\r\n'.join(lines) + '\r\n').encode('ascii')


def emit_geek(pages: list, out_dir: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / 'GeekSetup.txt').write_bytes(GEEK_SETUP_TXT.encode('ascii'))
    for idx, page in enumerate(pages, start=1):    # geek pages are 1-indexed
        pdir = out_dir / f'page{idx}'
        pdir.mkdir(exist_ok=True)
        leds = []
        for i in range(10):
            k = page.keys.get(i, Key(palette=0))
            (pdir / f'gekey{i}.dat').write_bytes(emit_geek_key(k))
            leds.append(str(k.palette))
        (pdir / 'keyled.dat').write_bytes(('\r\n'.join(leds) + '\r\n').encode('ascii'))


# --- Built-in pages (Katana, NDSP, Helix+Looper, FL, REAPER) -----------------

# Palette aliases for GeekMode keys (best inference from OEM defaults)
P_OFF, P_RED, P_GRN, P_BLU, P_YEL, P_MAG, P_ORG, P_PUR, P_CYN, P_PNK = range(10)
P_WW, P_CW = 15, 18


KATANA = Page('KTNA', encoder_cc=7, encoder_name='VOL',
              exp1_cc=1, exp2_cc=7,
              keys={
    0: Key('BOOST',  press=[cc(1,16,127)], release=[cc(1,16,0)], color=0x00ff00, palette=P_GRN, keytimes=2),
    1: Key('MOD',    press=[cc(1,17,127)], release=[cc(1,17,0)], color=0x0080ff, palette=P_BLU, keytimes=2),
    2: Key('DELAY',  press=[cc(1,19,127)], release=[cc(1,19,0)], color=0xffaa00, palette=P_ORG, keytimes=2),
    3: Key('REVRB',  press=[cc(1,20,127)], release=[cc(1,20,0)], color=0xaa00ff, palette=P_PUR, keytimes=2),
    4: Key('BANK+',  press=[bank_inc()], color=0xffffff, palette=P_OFF),
    5: Key('CH1',    press=[pc(1,0)], color=0x00ffff, palette=P_CYN, ledmode='select'),
    6: Key('CH2',    press=[pc(1,1)], color=0x00ffff, palette=P_CYN, ledmode='select'),
    7: Key('CH3',    press=[pc(1,2)], color=0x00ffff, palette=P_CYN, ledmode='select'),
    8: Key('TAP',    press=[cc(1,64,64)], color=0xff0000, palette=P_RED, ledmode='tap'),
    9: Key('BANK-',  press=[bank_dec()], color=0xffffff, palette=P_OFF),
})

NDSP = Page('NDSP', encoder_cc=7, encoder_name='OUT',
            exp1_cc=11, exp2_cc=7,
            keys={
    0: Key('BYPS',   press=[cc(1,100,0)], color=0x660000, palette=P_RED, ledmode='select'),
    1: Key('CHN1',   press=[cc(1,100,1)], color=0x00ff00, palette=P_GRN, ledmode='select'),
    2: Key('CHN2',   press=[cc(1,100,2)], color=0xffaa00, palette=P_ORG, ledmode='select'),
    3: Key('CHN3',   press=[cc(1,100,3)], color=0xff0000, palette=P_MAG, ledmode='select'),
    4: Key('BANK+',  press=[bank_inc()], color=0xffffff, palette=P_OFF),
    5: Key('ST1',    press=[cc(1,102,127)], release=[cc(1,102,0)], color=0x00ffff, palette=P_CYN, keytimes=2),
    6: Key('ST2',    press=[cc(1,103,127)], release=[cc(1,103,0)], color=0xff00ff, palette=P_MAG, keytimes=2),
    7: Key('ST3',    press=[cc(1,104,127)], release=[cc(1,104,0)], color=0xffff00, palette=P_YEL, keytimes=2),
    8: Key('TAP',    press=[cc(1,64,64)], color=0xff0000, palette=P_RED, ledmode='tap'),
    9: Key('BANK-',  press=[bank_dec()], color=0xffffff, palette=P_OFF),
})

HELIX = Page('HXLP', encoder_cc=7, encoder_name='VOL',
             exp1_cc=1, exp2_cc=11,
             keys={
    0: Key('SNP1',   press=[cc(1,69,0)], color=0x00ff00, palette=P_GRN, ledmode='select'),
    1: Key('SNP2',   press=[cc(1,69,1)], color=0xffaa00, palette=P_ORG, ledmode='select'),
    2: Key('SNP3',   press=[cc(1,69,2)], color=0xff0000, palette=P_RED, ledmode='select'),
    3: Key('SNP4',   press=[cc(1,69,3)], color=0xaa00ff, palette=P_PUR, ledmode='select'),
    4: Key('TUNR',   press=[cc(1,68,127)], release=[cc(1,68,0)], color=0xffffff, palette=P_WW, keytimes=2),
    5: Key('REC',    press=[cc(1,60,127)], color=0xff0000, palette=P_RED),
    6: Key('PLAY',   press=[cc(1,61,127)], release=[cc(1,61,0)], color=0x00ff00, palette=P_GRN, keytimes=2),
    7: Key('OVDB',   press=[cc(1,60,0)], color=0xffaa00, palette=P_ORG),
    8: Key('TAP',    press=[cc(1,64,64)], color=0xff0000, palette=P_RED, ledmode='tap'),
    9: Key('UNDO',   press=[cc(1,63,127)], color=0xffff00, palette=P_YEL),
})

FL = Page('FLSO', encoder_cc=7, encoder_name='VOL',
          exp1_cc=11, exp2_cc=7,
          keys={
    0: Key('PLAY',   press=[cc(1,102,127)], color=0x00ff00, palette=P_GRN),
    1: Key('STOP',   press=[cc(1,103,127)], color=0xff0000, palette=P_RED),
    2: Key('REC',    press=[cc(1,104,127)], release=[cc(1,104,0)], color=0xff0000, palette=P_RED, keytimes=2),
    3: Key('MTRO',   press=[cc(1,105,127)], release=[cc(1,105,0)], color=0xffaa00, palette=P_ORG, keytimes=2),
    4: Key('PAT+',   press=[cc(1,106,127)], color=0xffffff, palette=P_OFF),
    5: Key('SAVE',   press=[cc(1,107,127)], color=0x0080ff, palette=P_BLU),
    6: Key('UNDO',   press=[cc(1,108,127)], color=0xffff00, palette=P_YEL),
    7: Key('REDO',   press=[cc(1,109,127)], color=0xaaff00, palette=P_YEL),
    8: Key('TAP',    press=[cc(1,64,64)], color=0xff0000, palette=P_RED, ledmode='tap'),
    9: Key('PAT-',   press=[cc(1,110,127)], color=0xffffff, palette=P_OFF),
})

REAPER = Page('RPER', encoder_cc=7, encoder_name='VOL',
              exp1_cc=11, exp2_cc=7,
              keys={
    0: Key('PLAY',   press=[cc(1,111,127)], color=0x00ff00, palette=P_GRN),
    1: Key('STOP',   press=[cc(1,112,127)], color=0xff0000, palette=P_RED),
    2: Key('REC',    press=[cc(1,113,127)], release=[cc(1,113,0)], color=0xff0000, palette=P_RED, keytimes=2),
    3: Key('LOOP',   press=[cc(1,114,127)], release=[cc(1,114,0)], color=0x00ffff, palette=P_CYN, keytimes=2),
    4: Key('MRK+',   press=[cc(1,115,127)], color=0xffffff, palette=P_OFF),
    5: Key('SAVE',   press=[cc(1,116,127)], color=0x0080ff, palette=P_BLU),
    6: Key('UNDO',   press=[cc(1,117,127)], color=0xffff00, palette=P_YEL),
    7: Key('REDO',   press=[cc(1,118,127)], color=0xaaff00, palette=P_YEL),
    8: Key('TAP',    press=[cc(1,64,64)], color=0xff0000, palette=P_RED, ledmode='tap'),
    9: Key('MRK-',   press=[cc(1,119,127)], color=0xffffff, palette=P_OFF),
})


PAGES = [KATANA, NDSP, HELIX, FL, REAPER]


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument('--mode', choices=['super', 'geek'], required=True)
    ap.add_argument('--out', required=True)
    args = ap.parse_args()

    out = Path(args.out)
    if args.mode == 'super':
        emit_super(PAGES, out)
        print(f'wrote {len(PAGES)} SuperMode pages -> {out}/page0..{len(PAGES)-1}.txt')
    else:
        emit_geek(PAGES, out)
        print(f'wrote {len(PAGES)} GeekMode pages -> {out}/page1..{len(PAGES)}/ + GeekSetup.txt')


if __name__ == '__main__':
    main()

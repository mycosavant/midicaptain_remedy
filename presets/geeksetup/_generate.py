"""
Geek-mode preset generator for MIDI Captain.

Each key has 12 action slots (6 press + 6 release). A slot is a tuple:
    (channel, type, p1, p2)
where type is one of 'CC', 'NT', 'PC', 'UP', 'DW', '--'.

Pages are dicts of {key_index: {'press': [slots...], 'release': [slots...], 'led': palette_idx}}.
Empty/missing slots are filled with disabled (1, '--', 0, 0).

The .dat file format is: 12 records, each = 5 lines (channel, type, p1, p2, '-'),
with CRLF line endings to match the OEM-shipped files.
"""

from pathlib import Path

EMPTY = (1, '--', 0, 0)
SEP = '-'
CRLF = '\r\n'

# Palette indices (best inference from OEM defaults):
# 0 = off/dim (used for page-nav keys 4 and 9)
# 1 red, 2 green, 3 blue, 4 yellow, 5 magenta, 6 orange, 7 purple,
# 8 cyan, 9 pink, 15 warm white, 18 cool white
RED, GRN, BLU, YEL, MAG, ORG, PUR, CYN, PNK = 1, 2, 3, 4, 5, 6, 7, 8, 9
WW, CW, OFF = 15, 18, 0


def key(led, press=None, release=None):
    return {'led': led, 'press': list(press or []), 'release': list(release or [])}


def slots_to_lines(press, release):
    actions = (press + [EMPTY] * 6)[:6] + (release + [EMPTY] * 6)[:6]
    out = []
    for ch, ty, p1, p2 in actions:
        out.append(str(ch))
        out.append(ty)
        out.append(str(p1))
        out.append(str(p2))
        out.append(SEP)
    return CRLF.join(out) + CRLF


def write_page(folder: Path, page: dict):
    folder.mkdir(parents=True, exist_ok=True)
    leds = []
    for i in range(10):
        k = page.get(i, key(OFF))
        (folder / f'gekey{i}.dat').write_bytes(
            slots_to_lines(k['press'], k['release']).encode('ascii')
        )
        leds.append(str(k['led']))
    (folder / 'keyled.dat').write_bytes((CRLF.join(leds) + CRLF).encode('ascii'))


# Helper builders
def cc(ch, num, val): return (ch, 'CC', num, val)
def nt(ch, num, vel): return (ch, 'NT', num, vel)
def pc(ch, num):       return (ch, 'PC', num, 0)
def up():              return (1, 'UP', 0, 0)
def dw():              return (1, 'DW', 0, 0)


# =============================================================================
# PAGE 1 -- BOSS KATANA (GA-FC compatible CCs + PC channel select)
# =============================================================================
katana = {
    0: key(GRN, press=[cc(1, 16, 127)], release=[cc(1, 16, 0)]),         # BOOST
    1: key(BLU, press=[cc(1, 17, 127)], release=[cc(1, 17, 0)]),         # MOD
    2: key(ORG, press=[cc(1, 19, 127)], release=[cc(1, 19, 0)]),         # DELAY
    3: key(PUR, press=[cc(1, 20, 127)], release=[cc(1, 20, 0)]),         # REVERB
    4: key(OFF, press=[up()]),                                            # PAGE+
    5: key(CYN, press=[pc(1, 0)]),                                        # CH1
    6: key(CYN, press=[pc(1, 1)]),                                        # CH2
    7: key(CYN, press=[pc(1, 2)]),                                        # CH3
    8: key(RED, press=[cc(1, 64, 64)]),                                   # TAP
    9: key(OFF, press=[dw()]),                                            # PAGE-
}

# =============================================================================
# PAGE 2 -- NEURAL DSP ARCHETYPE (CC 100 chain select + learn-me stomps)
# =============================================================================
ndsp = {
    0: key(RED, press=[cc(1, 100, 0)]),                                   # BYPASS
    1: key(GRN, press=[cc(1, 100, 1)]),                                   # CHAIN1
    2: key(ORG, press=[cc(1, 100, 2)]),                                   # CHAIN2
    3: key(MAG, press=[cc(1, 100, 3)]),                                   # CHAIN3
    4: key(OFF, press=[up()]),                                            # PAGE+
    5: key(CYN, press=[cc(1, 102, 127)], release=[cc(1, 102, 0)]),        # STOMP1 (learn)
    6: key(MAG, press=[cc(1, 103, 127)], release=[cc(1, 103, 0)]),        # STOMP2 (learn)
    7: key(YEL, press=[cc(1, 104, 127)], release=[cc(1, 104, 0)]),        # STOMP3 (learn)
    8: key(RED, press=[cc(1, 64, 64)]),                                   # TAP
    9: key(OFF, press=[dw()]),                                            # PAGE-
}

# =============================================================================
# PAGE 3 -- HELIX NATIVE + CUSTOM LOOPER
# (CCs straight from Helix MIDI implementation chart)
# =============================================================================
helix = {
    0: key(GRN, press=[cc(1, 69, 0)]),                                    # SNAPSHOT 1
    1: key(ORG, press=[cc(1, 69, 1)]),                                    # SNAPSHOT 2
    2: key(RED, press=[cc(1, 69, 2)]),                                    # SNAPSHOT 3
    3: key(PUR, press=[cc(1, 69, 3)]),                                    # SNAPSHOT 4
    4: key(OFF, press=[up()]),                                            # PAGE+
    5: key(RED, press=[cc(1, 60, 127)]),                                  # LOOPER REC
    6: key(GRN, press=[cc(1, 61, 127)], release=[cc(1, 61, 0)]),          # LOOPER PLAY/STOP
    7: key(ORG, press=[cc(1, 60, 0)]),                                    # LOOPER OVERDUB
    8: key(RED, press=[cc(1, 64, 64)]),                                   # TAP
    9: key(OFF, press=[dw()]),                                            # PAGE-
}

# =============================================================================
# PAGE 4 -- FL STUDIO (transport via MIDI-Learn, CC 102-110)
# =============================================================================
fl = {
    0: key(GRN, press=[cc(1, 102, 127)]),                                 # PLAY
    1: key(RED, press=[cc(1, 103, 127)]),                                 # STOP
    2: key(RED, press=[cc(1, 104, 127)], release=[cc(1, 104, 0)]),        # REC toggle
    3: key(ORG, press=[cc(1, 105, 127)], release=[cc(1, 105, 0)]),        # METRONOME toggle
    4: key(OFF, press=[up()]),                                            # PAGE+
    5: key(BLU, press=[cc(1, 107, 127)]),                                 # SAVE
    6: key(YEL, press=[cc(1, 108, 127)]),                                 # UNDO
    7: key(YEL, press=[cc(1, 109, 127)]),                                 # REDO
    8: key(RED, press=[cc(1, 64, 64)]),                                   # TAP
    9: key(OFF, press=[dw()]),                                            # PAGE-
}

# =============================================================================
# PAGE 5 -- REAPER (transport via MIDI-Learn, CC 111-119)
# =============================================================================
reaper = {
    0: key(GRN, press=[cc(1, 111, 127)]),                                 # PLAY
    1: key(RED, press=[cc(1, 112, 127)]),                                 # STOP
    2: key(RED, press=[cc(1, 113, 127)], release=[cc(1, 113, 0)]),        # REC toggle
    3: key(CYN, press=[cc(1, 114, 127)], release=[cc(1, 114, 0)]),        # LOOP toggle
    4: key(OFF, press=[up()]),                                            # PAGE+
    5: key(BLU, press=[cc(1, 116, 127)]),                                 # SAVE
    6: key(YEL, press=[cc(1, 117, 127)]),                                 # UNDO
    7: key(YEL, press=[cc(1, 118, 127)]),                                 # REDO
    8: key(RED, press=[cc(1, 64, 64)]),                                   # TAP
    9: key(OFF, press=[dw()]),                                            # PAGE-
}


PAGES = {
    'page1': katana,
    'page2': ndsp,
    'page3': helix,
    'page4': fl,
    'page5': reaper,
}


def main():
    root = Path(__file__).parent
    for name, page in PAGES.items():
        write_page(root / name, page)
        print(f'wrote {name}/ (10 gekey + keyled)')


if __name__ == '__main__':
    main()

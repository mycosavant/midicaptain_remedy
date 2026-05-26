# MIDI Captain Preset Builder · webapp

Static, zero-dependency, dark-mode-only web editor for Paint Audio MIDI
Captain preset files. Lives entirely in `index.html` + `style.css` + `app.js`.

## Run locally

Any static server works. Two zero-install options:

```bash
# Python (3.x)
python -m http.server 5173 --directory webapp

# Node (built-in)
npx --yes serve webapp -l 5173
```

Then open <http://localhost:5173/>.

## Deploy

Just drop the three files into GitHub Pages, Cloudflare Pages, Vercel,
Netlify, or any static host. No build step.

## What it does

| Feature | Notes |
|---|---|
| SuperMode + GeekMode | Toggle in the top bar; layout, action limits, file format all adapt. |
| Device family selector | Captain STD/Blue-Gold (10), Mini (6), Nano (4). SVG layout and emitter both adapt. |
| 5 built-in presets | Katana, Neural DSP, Helix Native+Looper, FL Studio, REAPER -- editable. |
| SVG device graphic | Brand plate, ST7789 display showing live page name, encoder, LED-ringed switches. |
| Multi-action editor | 4 action states per key (press / release / long press / long release); up to 10 tap-cycle states in SuperMode; up to 6 stacked actions per state in GeekMode. |
| Per-state LED color | Each tap-cycle state has its own color, edited via state tabs. |
| Live text preview | Monospace pane on the right shows the exact file content that will be written. |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z (or topbar buttons). 60-entry ring buffer, debounced. |
| Import existing | Drag-in `page*.txt` (SuperMode) or `gekey*.dat` + `keyled.dat` (GeekMode). |
| Copy to clipboard | Copies the current page's text. |
| Download single file | `page<N>.txt` (SuperMode) or `gekey<N>.dat` (GeekMode). |
| Download all (ZIP) | All pages bundled. No deps -- uses a tiny built-in STORED-method zip writer. |
| Web MIDI live preview | Pick a MIDI output, hit TEST KEY to send the active key's bindings to your DAW/amp. Browser must support `navigator.requestMIDIAccess()` (Chrome / Edge / Opera). |
| i18n | EN / 中文 / FR / ES / PT. Persisted per user. |
| Reference panel | Verified CC tables for Katana / Helix / Neural DSP / FL / REAPER, plus macro-launcher tips. |
| Persistent state | Saved to `localStorage` on every edit. |

## Constraints honored

- **Dark mode only**, square corners (border-radius 0), monospace
  (`JetBrains Mono`) for technical content, Inter for UI labels.
- **Vanilla JS, zero npm deps.** Total external runtime weight = the two
  Google Fonts stylesheets (which are optional -- system fallbacks defined).
- **No build step.** Edit and refresh.

## File layout

```
webapp/
├── index.html        ~9 KB  -- semantic HTML, no inline JS
├── style.css        ~14 KB  -- dark theme, grid layout
├── app.js           ~55 KB  -- state, serializers, parser, zip writer, Web MIDI, i18n
└── README.md
```

## Real-time device sync

Web MIDI live preview sends messages to *any* MIDI output, including the
MIDI Captain itself if it's plugged in. That lets you hear/see what a
binding does in your DAW or amp before committing it.

**Writing config files to the device flash is a separate question.** The OEM
firmware requires you to hold Switch&nbsp;1 at boot to mount the USB MSC
drive. There's no way to host-side trigger that — key state at USB
enumeration is hardware-only. Real-time sync would need firmware-side
support (a USB Serial command or SysEx config protocol) on the device.
Right now: edit here → download → copy to the mounted drive → reboot.

## Sources

CC / PC / SysEx mappings cited inline in the reference panel:

- [Helix MIDI implementation chart](https://helixhelp.com/tips-and-guides/universal/midi)
- [Neural DSP MIDI control](https://neuraldsp.com/getting-started/controlling-plugins-with-midi)
- [FL Studio MIDI settings](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/envsettings_midi.htm)
- [REAPER MIDI mapping how-to](https://harmonicbuzz.com/mapping-a-midi-controller-to-reapers-transport-controls/)
- [Paint Audio (manufacturer)](https://www.pantheraudio.com/)

# Practical MIDI Extensions

1. MIDI Scene Controller - Store multiple "scenes" with complex multi-CC/PC messages triggered by single footswitch. Display shows scene name, LEDs indicate active layers.
2. MIDI Learn Mode - Press a button, wiggle a knob on your target device, and the footswitch auto-maps to that CC. No computer needed.
3. Setlist Manager - Load .txt files with song-specific MIDI mappings. Up/down scrolls through setlist, display shows song name, switches auto-reconfigure per song.
4. Looper Controller - Specialized firmware for controlling loopers (RC-500, Infinity, etc.) with visual feedback - display shows loop state, LEDs indicate recording/playing/overdub.

## Expression Pedal Enhancements

5. Expression Curves - Apply logarithmic, exponential, or custom response curves to expression pedals. Visual curve editor on display.
6. Expression Split/Merge - One expression pedal controls 2+ parameters with different ranges/curves. Or combine both pedals into one complex controller.
7. Auto-Wah/Tremolo Generator - LFO-driven CC output with adjustable rate/depth via encoder. Expression pedal becomes modulation depth.

## Performance Tools

8. Tap Tempo with Clock Output - Tap footswitch, device sends MIDI clock at detected BPM. Display shows BPM, LEDs pulse with beat.
9. Chord/Arpeggiator Trigger - Define chord voicings or arpeggios, footswitches trigger them. Good for synth/keyboard control.
10. Song Position Display - Receive MIDI clock, display current bar/beat. LEDs can flash on downbeat. Helpful for backing tracks.

## Display-Focused Ideas

11. Patch Name Display - Parse SysEx from your amp/modeler to show current preset name. Works great with HX Stomp, Kemper, etc.
12. Visual Metronome - Full-screen beat visualization synced to MIDI clock. Different patterns for different time signatures.
13. Lyrics/Notes Viewer - Load text files with lyrics or chord charts. Footswitches scroll through sections.

## Multi-Device/Protocol

14. USB HID + MIDI Hybrid - Simultaneously act as MIDI controller AND keyboard/mouse. Control DAW transport with keyboard shortcuts while sending MIDI.
15. OSC Gateway - If you add WiFi (external module), bridge MIDI to OSC for software like Resolume, QLab, or TouchDesigner.
16. DMX Lighting Controller - Add a DMX shield/module, control stage lighting alongside your MIDI rig.

## Advanced/Experimental

17. MIDI Sniffer/Debugger - Display incoming MIDI messages in real-time. Incredibly useful for troubleshooting live rigs.
18. Macro Sequencer - Record button press sequences and play them back. Automate complex MIDI routines.
19. Audio Analyzer (requires hardware mod) - Add I2S mic, display frequency spectrum or detect pitch for automatic harmony control.
20. Pedalboard Game - Guitar Hero style rhythm game using the footswitches. Fun for practice, and you've got the display for it.

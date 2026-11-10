# turzx-stats

A stats screen for the TURZX 9.2" USB panel (`1cbe:0092`, 1920x480) on cynthia. It reads the
numbers, draws one picture, sends it, and sleeps until the next update, so the processor can
idle in between. Written because Libre Panel kept the Ryzen at about 50 °C while idle.

Shows: time and date, CPU load and temperature with a history, memory, disk, the NVIDIA card
(load, power, VRAM), the model Ollama has loaded, and the Docker containers (stopped ones first,
in red).

## Build and run

```
cargo build --release
target/release/turzx-stats --png preview.png   # draw to a file, no panel needed
target/release/turzx-stats --once              # one picture on the panel
target/release/turzx-stats --interval 5 --brightness 60
```

Needs `DejaVuSans.ttf` and `DejaVuSans-Bold.ttf` (Debian: `fonts-dejavu-core`; `--fonts DIR`
for another place), write access to the panel's USB device (see below), and, for the last two
cards, the Ollama port on 127.0.0.1:11434 and membership of the `docker` group. Whatever cannot
be read says so on the screen.

Stopping it (SIGTERM, Ctrl+C) turns the panel dark, so a stopped screen never shows old numbers.

## On cynthia

- Binary: `~/apps/turzx-stats/turzx-stats`, built on dawn and copied over (same Debian 13).
- USB access: `/etc/udev/rules.d/61-turzx-plugdev.rules` gives the panel to group `plugdev`.
- Service: `deploy/turzx-stats.service` goes to `~/.config/systemd/user/`; then
  `systemctl --user enable --now turzx-stats` and once `sudo loginctl enable-linger ish`.

## Measured on cynthia (2026-11-10, idle, one picture every 5 s)

| | Libre Panel 0.8.0 | turzx-stats |
|---|---|---|
| Share of one core | 8-31 % | 0.3 % |
| Wake-ups per second | about 40 | 1.5 |
| Memory (resident) | 100-140 MB | 69 MB |
| CPU temperature | 44-56 °C | 26-44 °C, mostly 26-31 |

## Protocol

From the notes in [libre-panel](https://github.com/syntensa/libre-panel)
(`docs/protocol/turzx-usb.md`, GPL-3.0), which is why this is GPL-3.0-or-later too. In short:
512-byte command packets, DES-CBC with a fixed key, the PNG (RGBA, 480x1920) following the
packet in the same bulk transfer. Only three commands are ever sent: hello (10), brightness (14)
and PNG frame (102). Nothing is written to the panel's storage or firmware.

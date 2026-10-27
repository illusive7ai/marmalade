# Marmalade

**A lightweight Windows desktop cat that does your repetitive everyday tasks.**

Marmalade is a click-through overlay cat that lives on your desktop. Talk to it (or type) and it handles the small, repetitive jobs you’d rather not do by hand — finding files, converting documents, printing, scanning, cleaning up, and more — then shows you what it did with a short animation.

It is designed to stay out of the way: tiny memory footprint, near-zero CPU when idle, no telemetry, and no account required.

## Features (current & planned)

- **Instant file search** — “the Excel I edited last week”, recent files, open containing folder
- **File operations** — convert (HEIC→JPG, DOCX→PDF, video→GIF/MP3…), compress, rename, zip/unzip, find duplicates
- **Printing & scanning** — print with options, scan with Windows Defender, check downloads/attachments
- **System helpers** — free disk space, clear temp, fix Wi-Fi, toggle dark mode / DND, startup apps
- **Daily productivity** — clipboard history, OCR, reminders, timers, weather-aware moods
- **Safety-first** — confirmation for destructive actions, undo log, local-only by default

The cat itself is the interface: hotkey or voice, then an animation shows the action (fetching a file, carrying it back, reacting to results).

## Why Marmalade?

Most desktop assistants are heavy, cloud-dependent, or focused on chat. Marmalade is a local, low-resource companion built specifically for the boring repetitive tasks that still take time every day. The cat is the friendly front-end; the real work happens in a small, fast Rust service.

## Status

Early development (v0.0.1). Core overlay, sprite player, chat bar, and command routing are in place. File search, conversions, and scanning are next.

Measured on Windows 11 (placeholder sprites):

| State     | Private memory | CPU     |
|-----------|----------------|---------|
| Awake     | ~5 MB          | < 0.3 % |
| Asleep    | ~2.5 MB        | 0 %     |

## Building

Requires the stable Rust toolchain.

```bash
cargo build --release

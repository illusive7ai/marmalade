# Day-one spike results: native Rust overlay

Build: `cargo build --release` (Rust 1.99.1, windows 0.58, tiny-skia 0.11). Binary: ~496 KB.
Measured with `scripts/measure.ps1` on Windows 11 Pro, one run.

| State | Private MB | Working set MB | CPU % (all cores) | Threads |
|---|---|---|---|---|
| Awake (30 fps breathing and blinking) | 1.4 | 7.7 | 0.41 | 4 |
| Asleep (timer killed, blocked in GetMessageW) | 1.3 | 7.6 | 0.00 | 1 |

Notes and limits:
- Working set includes shared system DLL pages. Private memory is the cost this process adds.
- Threads are a snapshot count, not a wakeup rate. Wakeups per second were not measured yet (needs ETW/WPR or Process Explorer).
- Single run, single machine, 160x160 overlay, no UI text, no search index. Treat as a floor, not a final number.
- Not yet compared against Tauri 2. That comparison is still needed to confirm the stack choice with numbers.

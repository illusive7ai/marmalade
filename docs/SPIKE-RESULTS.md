# Spike results: native Rust overlay

Measured with `scripts/measure.ps1` on Windows 11 Pro. Single machine, single run each, so treat
these as floors, not final numbers. Budgets are enforced by `measure.ps1 -Enforce` (also run in CI,
non-blocking until the hosted runner is confirmed to create the overlay window).

## Run 1: code-drawn placeholder cat (tiny-skia, 160x160, fixed 30 fps timer)

Binary ~496 KB.

| State | Private MB | Working set MB | CPU % (all cores) | Threads |
|---|---|---|---|---|
| Awake (30 fps) | 1.4 | 7.7 | 0.41 | 4 |
| Asleep (timer killed, blocked in GetMessageW) | 1.3 | 7.6 | 0.00 | 1 |

## Run 2: sprite player with placeholder frames (256x256, 12 fps walk, event-driven timer)

Frames are decoded from PNG by `marmalade-sprites`; the timer is armed from `wake_in()`, so it
fires only when a frame is due. Binary ~526 KB. The `walk` placeholder is 12 frames, about 3 MB decoded.

| State | Private MB | Working set MB | CPU % (all cores) | Context switches/s | Threads |
|---|---|---|---|---|---|
| Awake (12 fps walk) | 4.4 | 10.5 | 0.22 | 26 | 4 |
| Asleep (1 frame kept, no timer) | 2.4 | 8.2 | 0.00 | 0 | 1 |

Takeaways:
- Event-driven frame timing cut awake context switches from ~46/s (30 fps) to ~26/s (12 fps).
- Asleep is truly idle: 0% CPU and 0 context switches per second across all threads.
- Awake memory is dominated by decoded frames (12 x 256 KB = 3 MB), as the asset spec predicted.
- Asleep private memory (2.4 MB) is higher than the ~1.4 MB baseline plus one frame (0.25 MB). The freed
  frames are likely still held by the allocator rather than returned to the OS. Follow-up: try
  `HeapCompact` after `sleep()`, or decode into a dedicated heap that can be destroyed.

## Run 3: the app with the chat bar and command router (bar created but hidden)

The spike became the real app (`crates/marmalade-app`, binary `marmalade.exe`, 577 KB). The chat bar is a
native EDIT control in a rounded dark window, plus `marmalade-core` for routing.

| State | Private MB | Working set MB | CPU % (all cores) | Context switches/s | Threads |
|---|---|---|---|---|---|
| Awake (12 fps walk) | 4.7 | 10.8 | 0.19 | 27 | 4 |
| Asleep | 2.4 | 8.5 | 0.00 | 0 | 1 |

The hidden bar adds about 0.3 MB awake and nothing measurable asleep. Not yet measured: memory and CPU
while the bar is open and while typing.

End-to-end check (real hotkey and keystrokes, with screenshots reviewed): Ctrl+Alt+Space opened the bar and
focused the input; the live preview showed "Find PDF files from the last week"; Enter ran the stub and cleared
the input; `print invoice.pdf` asked for a second Enter; Esc cancelled the confirmation, a second Esc closed the bar.

## Budgets (scripts/measure.ps1)

| Metric | Budget |
|---|---|
| Private memory, awake or asleep | 25 MB |
| Awake CPU | 1.0% |
| Asleep CPU | 0.05% |
| Asleep context switches/s (wakeup proxy) | 2 |

## Limits

- Working set includes shared system DLL pages; private memory is the cost this process adds.
- Context switches are a proxy for wakeups, not a true ETW wakeup trace.
- No UI text, no search index, and no realistic cat frames yet. Real frames will cost more per animation.
- Rendering was verified by unit tests (blit, mirroring, clipping) and by memory use, not by a screenshot.
- Not yet compared against Tauri 2. That comparison is still needed to confirm the stack choice with numbers.

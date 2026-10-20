# Project Ideas (saved 2026-10-20)

## 1. Goal
Build a GitHub project that gains stars quickly and solves everyday user problems.
Domains: technology, cybersecurity, design, development.

## 2. GitHub trends observed (this month)
- Local/open-source replacements for paid AI tools (VoiceStudio ~44k stars, freellmapi ~10k).
- Tooling around coding agents: skills, plugins, memory, context optimization, security audits
  (hindsight, ECC, context-mode, cloudflare/security-audit-skill, claude-plugins-official).
- Design/diagram tools for AI output (impeccable, diagram-design).
- Reverse engineering and security (Ghidra).
- Caution: morluto/rea showed 201k stars in a week, likely botted or a data glitch. Ignore as signal.

## 3. Top-level project options
1. Local-first "paid tool killer" with a simple UI (highest star odds).
2. Agent-safety / security scanner for Claude Code, Cursor etc. (skills, plugins, MCP servers, prompt injection, secret leaks).
3. Windows-first privacy/security tool (offline leak checker, tracker blocker, file/link safety checker).
4. "AI slop detector" for UI design (lints generic AI-looking UI).

## 4. Zoom: paid forensic tools killer
Gap: open-source stack exists (Autopsy/Sleuth Kit, Volatility 3, Plaso + Timesketch, Velociraptor)
but is fragmented and CLI-heavy. Paid suites (Magnet AXIOM, X-Ways, Cellebrite, Belkasoft) win on polish.
- Unified DFIR workbench: GUI over existing parsers, one timeline, search, tagging, one-click report. (best bet)
- Artifact timeline and HTML report generator.
- Windows triage collector (KAPE/Velociraptor-lite) with offline viewer.
- Avoid mobile extraction (Cellebrite-style): legally and ethically murky.
- Risks: trust (hash everything, read-only, log actions), legal disclaimer, scope creep (wrap parsers, do not rewrite).

## 5. Reference: Coucou (github.com/Louis-CFM/coucou, ~5.6k stars)
- Mascot "Mochi", a squircle with big eyes that pops out of the Mac notch. Idle breathing, blinks,
  follows the cursor, reacts when poked, jumps when tasks finish, wardrobe, floats on desktop.
- Animation drawn in code (SwiftUI Canvas / Canvas 2D), no Rive/Lottie/image assets.
- 28 handcrafted sounds. Colored pills per service. State colors: orange = waiting on you, blue = working, green = done.
- Windows/Linux built with Tauri 2 (Rust + TypeScript).
- Privacy: no telemetry, no account, keys in OS credential manager, "never blocks the user" if not running.
- Name, mascot, icon and sounds are reserved separately from the MIT code. Do NOT copy the art or name.

## 6. Reference: desktop-cat-linux-prompt.md
Location: C:\Users\User\OneDrive\Documents\desktop-cat-linux-prompt.md
- Realistic cat (sprite frames), click-through overlay except cat and chat bar.
- Animations: lying, sitting, walking, standing on hind legs, crouch and leap, peeking, carrying a file, paw on button.
- Behaviors: roam, walk along window edges, resize a window, close a window (graceful, explicit only), hotkey chat bar,
  file fetch scene (cat jumps into a folder icon and brings the file back).
- Safety: never close/move/resize windows unless the user explicitly triggers it; no network by default.
- Windows port equivalents: layered click-through window, RegisterHotKey, WM_CLOSE, SetWinEventHook / DwmGetWindowAttribute.

## 7. Main concept: a Windows-first desktop cat that automates daily repetitive tasks by voice/chat
Working names (not Coucou/Mochi): Whiskr, Kibble, Tabby.
Cat is the interface: hotkey or hold-to-speak, then the cat does the task and shows it with an animation.

### Core requirements
- Windows first.
- Lightweight, very fast, memory efficient.
- Aesthetics in Coucou's style (procedural, pills, state colors, sounds).

### Files
- Instant search by name/content/type/date ("the Excel I edited last week").
- Recent files, where did my download go, open containing folder.
- Duplicate finder, biggest files, clean Downloads, organize by type, bulk rename, zip/unzip, restore from Recycle Bin.

### Conversions
- HEIC to JPG, PNG to WebP, resize/compress images.
- DOCX to PDF, PDF merge/split/compress.
- Video to MP4/GIF, video to MP3, shrink screen recordings.
- OCR image to text.

### Printing
- "Print this" with copies, duplex, B/W, page range.
- List printers, set default, print to PDF, clear stuck print queue.

### Scanning and safety
- Scan files/folders with Windows Defender (MpCmdRun), no bundled engine.
- Check attachments/downloads: double extensions, Mark-of-the-Web, Office macros, risky .lnk/.iso/scripts.
- VirusTotal hash lookup with the user's own key (hash only).
- Auto-check USB drives on insert. Link checker. HIBP k-anonymity password breach check.
- Strip metadata before sharing. Physical scanning over WIA to PDF.

### System helper
- Free disk space, clear temp, "why is my PC slow", kill frozen app, fix Wi-Fi (DNS flush, adapter reset).
- Toggle dark mode / do-not-disturb / volume / brightness, shutdown timer, startup apps manager.

### Daily productivity
- Clipboard history, snippets, screenshot + annotate + OCR, dictation (hold key, speak, paste),
  summarize/translate selected text, QR code to send file to phone, meeting notetaker.

### Standout ideas (my picks)
1. "Where was that?" memory: tiny local log of recently opened files/windows/downloads; ask "the PDF I was reading before lunch".
2. Drag a file onto the cat: context buttons per file type (Convert, Print, Scan, Compress, Send to phone).
3. Pre-flight check before sharing: hidden metadata, tracked changes/comments, GPS in photos, size limits, wrong filename.
4. Download guardian: auto-sort, scan and describe new downloads.
5. Panic button: "someone's coming" hides windows, mutes, locks.
6. Fix-it mode: guided diagnostics for internet, printer, slow PC, Bluetooth.
7. Undo for everything: every action is logged and reversible.

### Security extras
- Fake-login/phishing guard, clipboard hijack alert (crypto address swap), persistence watcher (startup/tasks/services),
  one-drag attachment verdict in plain language, honeyfile ransomware tripwire, camera/mic usage dashboard.

### Weather and environment (from user)
- Weather moods: umbrella for rain, sunglasses for sun, scarf for snow, hides in storms, "drink water" in heat.
- Time-of-day behavior (sleeps at night, stretches in morning), seasons and holidays outfits, "rain in 20 min" nudges.

### Games (from user, expanded)
- Chase the cursor, catch the mouse on the taskbar, fetch with a yarn ball (physics), cat vs bugs on the desktop,
  daily 30-second challenge with streak, break-time game when working too long.

### Time and reminders (from user, expanded)
- Voice alarms/timers, smart reminders (trigger by Wi-Fi network or app opened), recurring habits (water, stretch, 20-20-20, posture, medication),
  meeting heads-up, birthday/bill tracker, focus mode (block distracting apps).

### Care and companionship
- Pet/feed (earn kibble from completed tasks), levels unlock outfits/tricks, greets on login and says goodnight,
  worried when battery low or disk nearly full.

### Everyday helpers
- Daily briefing pill, currency/unit/tip/split calculators, translate and grammar help, password generator,
  shopping/notes list, clipboard tricks (format as table, make polite, shorten), read selected text aloud.

### Social and sharing
- Shareable cat card (level, time saved), community cat/outfit/sound packs, gift a cat via link.
- Receipt pill after each task ("converted 12 images, saved 48 MB"), weekly recap.

### Accessibility
- Large text, high contrast, full keyboard control, voice-only operation, calm mode (no sound, minimal motion), silent mode in meetings/fullscreen.

### Developer and power-user
- Plugin system (small manifest file adds a command).
- MCP server exposing find/convert/print/scan so Claude Code and other agents can use the cat as a safe file tool.
- CLI mode, e.g. `cat find "invoice"`.

### Explicitly excluded
- Windows/Office activation via massgrave/MAS: piracy tool (license circumvention), flagged by Defender, repo takedown risk.
  Legit alternative: show activation status/license type, point to free alternatives (LibreOffice, Office on the web).

## 8. Architecture recommendations
- Two parts: `catd` background service in Rust (~5 MB) for search/convert/print/scan; separate overlay UI process that sleeps or exits when idle.
- Instant search via NTFS USN journal / MFT (Everything-style), not folder crawling.
- Use existing tools on demand: ffmpeg/pandoc as optional downloads, Defender for scans, Windows print API,
  Word/LibreOffice for DOCX to PDF if installed.
- UI stack: native Rust layered click-through window (~10-20 MB idle, recommended) vs Tauri 2 (faster to build, WebView2 ~50-100 MB).
- Cat style: stylized cat drawn in code (tiny memory, recommended) vs realistic sprite-based cat (heavier, needs supplied frames).
- Command understanding: fast local rules first (instant, offline); LLM only for fuzzy requests via Ollama or user's own API key.
- Safety: LLM only picks from a fixed action list, never runs a shell (prompt injection via filenames/doc text);
  destructive actions require confirmation; never close/move/resize windows without explicit user action.
- Privacy: no telemetry, no account, keys in Windows Credential Manager.

## 9. Suggested MVP (about a weekend)
1. Click-through cat overlay with idle/sleep states.
2. Hotkey chat bar.
3. Instant file search + fetch animation (the viral GIF).
4. Defender scan of any file.
5. Print a file.
6. Image and PDF conversion.
Then: demo GIF, one-command install, README with comparison table.
Suggested first-version extras: weather moods, cursor-chase game, voice alarms/reminders, cat card,
drag-a-file-onto-cat, pre-flight share check, undo, MCP server.

## 10. Star-growth advice
- README, demo GIF and launch matter more than code. 15-second GIF, one-line install, clear "why not X?" section.
- Launch on Hacker News (Show HN), Reddit (r/selfhosted, r/LocalLLaMA, r/netsec), X.
- Ship a working v0.1 in days. No one can promise thousands of stars.

## 11. Open decisions
- UI stack: native Rust (recommended) or Tauri.
- Cat style: stylized in-code (recommended) or realistic sprites.
- Name: Whiskr / Kibble / Tabby / other.

<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="112" alt="BrainTracker logo" />

# BrainTracker

**For the things you'll forget — and need again someday.**

A tiny, fast, offline-first Windows app for jotting down what you just did: the fix, the command,<br/>
the setting you changed, the screenshot that explains it. Write it in seconds, find it again in seconds.

[![Latest release](https://img.shields.io/github/v/release/ioannismil/braintracker?style=flat-square&color=d9603b&label=latest)](https://github.com/ioannismil/braintracker/releases/latest)
[![Release build](https://img.shields.io/github/actions/workflow/status/ioannismil/braintracker/release.yml?branch=main&style=flat-square&label=build)](https://github.com/ioannismil/braintracker/actions/workflows/release.yml)
[![Downloads](https://img.shields.io/github/downloads/ioannismil/braintracker/total?style=flat-square&color=f2a65a)](https://github.com/ioannismil/braintracker/releases)
![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-2b2420?style=flat-square)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24c8db?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![License: MIT](https://img.shields.io/badge/license-MIT-6b5f56?style=flat-square)](LICENSE)

[**Download**](#-download) · [Features](#-features) · [Shortcuts](#%EF%B8%8F-keyboard-shortcuts) · [Command line](#-command-line) · [Your data](#-your-data) · [Build from source](#%EF%B8%8F-build-from-source)

</div>

---

## 🧠 What it is, and what it isn't

You fix something tricky, and six months later it breaks again. You remember fixing it, but not *how*. BrainTracker is for that moment: a place to drop the details you'd otherwise forget, so you never have to work them out twice.

It's deliberately **not** a knowledge base or a "second brain". Apps like Obsidian or Notion are great for building connected notes you organise and come back to on purpose. BrainTracker asks for none of that: no folders, no links between notes, no structure to keep up. You write a few lines while it's fresh, add a tag if you feel like it, and move on. When you need it later, you search and it's there.

Think of it as a memory you can search, not a library you keep tidy.

## ✨ Features

<table>
<tr>
<td width="50%" valign="top">

### ⚡ Capture in seconds
Press <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>E</kbd> anywhere — a small popup appears on top of whatever you're doing. Type what you did, hit <kbd>Enter</kbd>, done. Click away and your half-written note is kept. Select some text in any app and press <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>D</kbd> instead to drop it straight into the notes of a new entry.

</td>
<td width="50%" valign="top">

### 🔎 Find anything instantly
Full-text search across titles, notes and tags, as you type, with highlighted matches. Partial words match, so `dock` finds *Docker*. Narrow it down with filters like `tag:docker`, `has:image` or `after:2026-10`. Or press <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Space</kbd> anywhere for a Spotlight-style search bar that shows your notes as you type. Staring at an error? Select it and press <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> to search for it; if no note has every word, you get the closest matches.

</td>
</tr>
<tr>
<td valign="top">

### 📝 Notes that hold the "how"
Markdown with code blocks (each with a one-click **Copy** button), lists and links. Write blanks into a command, like `ssh {{user}}@{{host}} -p {{port:22}}`, and **Copy** asks for the values first, remembering them for next time (see **Settings → Commands**). Paste screenshots straight in with <kbd>Ctrl</kbd> <kbd>V</kbd>, and the text in them becomes searchable too (read offline by Windows' built-in OCR), so searching an error code finds the screenshot of that error. Everything auto-saves as you type.

</td>
<td valign="top">

### 🗂️ Easy to navigate
Entries grouped by *Today · Yesterday · This week · month*. Pin the important ones to the top. Browse with the arrow keys without touching the mouse. **Pop out** a note into a small window that stays on top of other apps, to follow its steps while you work.

</td>
</tr>
<tr>
<td valign="top">

### 🖥️ Always there, never in the way
Starts with Windows and lives in the system tray. Summon the full window with <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd>. Closing just hides it.

</td>
<td valign="top">

### 🔒 Yours, and offline
No account, no cloud, no telemetry. One local SQLite file you can export to plain Markdown anytime. Every shortcut is configurable. Turn on **offline mode** and BrainTracker makes no network connections at all: no update checks, no web images.

</td>
</tr>
<tr>
<td valign="top">

### 🛟 Automatic backups
A daily snapshot whenever something changed — to `Documents`, OneDrive or any folder you pick. Keeps the newest 30; restore any of them in one click.

</td>
<td valign="top">

### 🔄 Updates itself
Checks GitHub for new releases and installs them with one click — your data and settings carry over. Portable copies get a download link instead.

</td>
</tr>
<tr>
<td valign="top">

### ↺ Nothing is lost by accident
Deleting moves an entry to the **Trash** for 30 days (with an instant *Undo*). Every entry keeps an **edit history** — step back to any earlier version.

</td>
<td valign="top">

### 📦 Bring your notes with you
**Import** folders of Markdown or text notes — titles, tags, dates and images included. Round-trips cleanly with BrainTracker's own Markdown export. Or switch on the **inbox folder**: anything you save into it (say, a OneDrive folder, from your phone) becomes an entry automatically.

</td>
</tr>
</table>

## 📥 Download

Grab the latest version from the **[Releases page](https://github.com/ioannismil/braintracker/releases/latest)**:

| | File | Best for |
|---|---|---|
| 🧩 **Installer** | `BrainTracker_x.y.z_x64-setup.exe` | Everyday use — Start menu entry, launches at startup, no admin rights needed |
| 🎒 **Portable** | `BrainTracker_x.y.z_x64_portable.zip` | No install — unzip anywhere (even a USB stick); data stays inside the folder |

> [!NOTE]
> BrainTracker isn't code-signed yet, so Windows SmartScreen may say *"Windows protected your PC"*. Click **More info → Run anyway**.
> It uses the Microsoft Edge WebView2 runtime, which is already installed on Windows 11 and up-to-date Windows 10.

**Updating:** automatic — BrainTracker shows *"Update available"* in the sidebar; click **Install & restart**. (You can also check from **Settings → Updates**.) Portable users download the new zip and copy their `data` folder over.

## ⌨️ Keyboard shortcuts

| Action | Default | Where |
|---|---|---|
| Show / hide BrainTracker | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> | 🌐 anywhere |
| Quick capture | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>E</kbd> | 🌐 anywhere |
| Quick capture with the selected text | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>D</kbd> | 🌐 anywhere |
| Quick search | <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Space</kbd> | 🌐 anywhere |
| Quick search for the selected text | <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> | 🌐 anywhere |
| New entry | <kbd>Ctrl</kbd> <kbd>T</kbd> | in the app |
| Search | <kbd>Ctrl</kbd> <kbd>K</kbd> | in the app |
| Toggle Markdown preview | <kbd>Ctrl</kbd> <kbd>E</kbd> | in the app |
| Settings | <kbd>Ctrl</kbd> <kbd>,</kbd> | in the app |

All of these can be changed in **Settings → Keyboard shortcuts** — click one, press the new keys. Your choices are kept when BrainTracker updates; a shortcut added in a new version never takes keys you already use. Shortcuts follow the physical key, so they work on any keyboard layout.

### 🔎 Search filters

Type these in the search box, on their own or together with words:

| Filter | Finds entries… |
|---|---|
| `tag:docker` | tagged *docker* (repeat for several tags) |
| `after:2026-10` · `before:2026` | created on/after or before a year, month or day |
| `on:2026-11-17` · `on:2026-11` | created on that day / in that month or year |
| `has:image` · `has:code` · `has:link` | with a screenshot, code, or a web link |
| `is:pinned` | that are pinned |

Example: `vpn tag:home after:2026-01 has:code`

<details>
<summary><b>More keys</b></summary>

| Where | Keys |
|---|---|
| Search box | <kbd>↑</kbd> <kbd>↓</kbd> browse results · <kbd>Enter</kbd> open · <kbd>Enter</kbd> with no results creates an entry with that title · <kbd>Esc</kbd> clear |
| Quick capture | <kbd>Enter</kbd> save (stays open for the next one) · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Enter</kbd> save & close · <kbd>Shift</kbd>+<kbd>Enter</kbd> jump to notes · <kbd>Ctrl</kbd>+<kbd>O</kbd> open app · <kbd>Esc</kbd> close — all changeable in Settings |
| Quick search | type to find notes · <kbd>↑</kbd> <kbd>↓</kbd> choose · <kbd>Enter</kbd> show the whole note · <kbd>Enter</kbd> again or <kbd>Ctrl</kbd>+<kbd>Enter</kbd> open it in BrainTracker · <kbd>Ctrl</kbd>+<kbd>P</kbd> pop it out on top · <kbd>Esc</kbd> back / clear / close |
| Popped-out note | <kbd>Esc</kbd> close · **📌 On top** toggles staying above other apps · **✎ Edit** opens it in BrainTracker |
| Editor | <kbd>Tab</kbd> indents · <kbd>Ctrl</kbd>+<kbd>V</kbd> pastes screenshots · double-click the preview to edit |

</details>

## 💻 Command line

Everything works from the terminal too — straight on your notes, without opening a window — so you can script it. On Windows use **`bt`**, installed next to BrainTracker (on Linux/macOS, `braintracker` works the same).

```powershell
bt search docker dns                 # find notes (filters work: tag:wsl has:code after:2026-10)
bt list --tag docker                 # recent notes
bt show 12                           # print a note in full
bt add "Fixed DNS in WSL" --tags wsl,network --body "Set nameserver 1.1.1.1"
git log -1 | bt add "Today's release" --body -      # - reads the text from stdin
bt edit 12 --append "Also needed a restart"
bt pin 12 · bt unpin 12 · bt delete 12 · bt tags
bt list --json                       # machine-readable output for scripts
```

Add **`--open`** to do it in the app instead:

| Command | Opens |
|---|---|
| `bt --open` | BrainTracker |
| `bt search [words] --open` | Quick search (with those words typed in) |
| `bt search --open --selection` | Quick search for the text selected in the app in front |
| `bt add --open` | Quick capture |
| `bt add --open --selection` | Quick capture with the text selected in the app in front |
| `bt show 12 --open` | Note #12 in BrainTracker |

That makes them easy to bind to a desktop shortcut, a launcher, or your desktop's own keyboard settings. `bt --help` lists everything.

To type just `bt` in any terminal, add BrainTracker's folder to your PATH once (PowerShell):

```powershell
[Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$env:LOCALAPPDATA\BrainTracker", "User")
```

## 💾 Your data

Everything stays on your machine.

| | Location |
|---|---|
| Installed | `%APPDATA%\com.braintracker.app\` |
| Portable | `data\` next to `BrainTracker.exe` |

Inside you'll find `braintracker.db` (all entries, plus the text read from your screenshots) and `images\` (pasted screenshots).

**Text in screenshots** is read in the background by Windows' own OCR. Nothing is uploaded. Screenshots from before this feature are read the first time the app starts. It uses the OCR languages installed in Windows (**Settings → Time & language → Language & region**); with none, screenshots simply aren't searchable.

**Backups** run automatically once a day when something changed, into `Documents\BrainTracker Backups` (portable: `data\backups`). Change the folder, how many to keep, or restore one in **Settings → Automatic backups**. A restore always saves a *before-restore* snapshot of your current data first, and keeps your current settings.

**Trash & history:** deleted entries wait in the **Trash** (sidebar, bottom) for 30 days. While you edit, a version is saved at most every 10 minutes (last 50 per entry) — open **↺ History** in the editor to compare and restore.

**Export & import:** **Settings → Export** writes every entry as a Markdown file — screenshots included — to `Documents\BrainTracker Export`. **Settings → Import** reads Markdown/text files or whole folders back in (titles, tags, dates, front matter and local images), skipping anything already imported.

**Inbox folder:** **Settings → Inbox folder** watches a folder (by default `Documents\BrainTracker Inbox`) and imports Markdown/text files and images put into it, tagged `#inbox`, then moves them into its `Imported` subfolder. A file is only imported once it has stopped changing, so files still being synced are left alone. Pick a folder inside OneDrive, Dropbox or Google Drive to capture from your phone through that service's app.

**Offline mode:** **Settings → Offline mode** stops every network connection BrainTracker makes: update checks and web images in notes (shown as a placeholder instead). Links still open in your browser when you click them.

**Clean up:** **Settings → Clean up screenshots** removes pasted images no entry, trashed entry or old version still uses.

> [!TIP]
> Any copy of `BrainTracker.exe` becomes portable when a file named `portable.txt` sits next to it.

## 🛠️ Build from source

**Requirements:** [Node.js 20+](https://nodejs.org), [Rust (stable)](https://rustup.rs), Windows 10/11.

```powershell
git clone https://github.com/ioannismil/braintracker.git
cd braintracker
npm install

npm run app:dev       # run with hot reload
npm run app:build     # build the installer → src-tauri/target/release/bundle/nsis/
cargo test --manifest-path src-tauri/Cargo.toml   # run the tests
```

**End-to-end tests** drive the real, built app the way a person would: every window, button and keyboard shortcut, plus `bt` while the app is open. They run against a portable copy with its own empty data, so your notes are never touched.

```powershell
cargo install tauri-driver --locked   # once
npx tauri build --no-bundle           # build the app to test
npm run e2e                           # everything; presses real keys, so don't type while it runs
npm run e2e -- --stop-app             # same, quitting your running BrainTracker first (restarted after)
npm run e2e -- --ci                   # skip tests that need a real desktop
```

Screenshots of any failures go to `e2e/out/`. They aren't run by the release workflow (desktop tests don't suit GitHub's Windows machines), so run them before pushing app changes.

<details>
<summary><b>Project structure</b></summary>

```
braintracker/
├─ src/                      React + TypeScript UI
│  ├─ App.tsx                main window: sidebar, list, editor
│  ├─ Capture.tsx            quick-capture popup
│  ├─ Settings.tsx           settings dialog & shortcut recorder
│  ├─ Backups.tsx · DataTools.tsx   backups, import, image cleanup
│  ├─ Trash.tsx · History.tsx       trash and edit-history dialogs
│  ├─ markdown.ts            Markdown rendering + code copy buttons
│  ├─ shortcuts.ts           layout-independent shortcut matching
│  └─ images.ts              screenshot paste / rendering
├─ src-tauri/                Rust backend (Tauri 2)
│  ├─ src/db.rs              SQLite schema, FTS5 search & filters, trash, history
│  ├─ src/backup.rs          daily backups & restore
│  ├─ src/import.rs          Markdown/text import
│  ├─ src/updates.rs         auto-update from GitHub Releases
│  ├─ src/lib.rs             commands, tray, global shortcuts, autostart, portable mode
│  └─ tauri.conf.json        windows, bundle & version (major.minor)
├─ e2e/                      end-to-end tests that drive the real app (npm run e2e)
├─ scripts/set-version.mjs   stamps the release version into all manifests
└─ .github/                  CI, release pipeline, Dependabot
```

</details>

**Tech stack:** [Tauri 2](https://tauri.app) · Rust · [SQLite](https://sqlite.org) with FTS5 via [rusqlite](https://github.com/rusqlite/rusqlite) · React 19 · TypeScript · Vite · [marked](https://marked.js.org) + [DOMPurify](https://github.com/cure53/DOMPurify)

## 🚀 Releases are automatic

Every push to `main` runs the [Release workflow](.github/workflows/release.yml), which:

1. runs the test suite,
2. builds the app and the NSIS installer,
3. packages the portable zip,
4. signs the installer and publishes `latest.json`, which installed copies use to update themselves,
5. publishes a GitHub release tagged **`v<major>.<minor>.<build>`** with a changelog generated from the commits since the previous release.

- To start a new version series, change `major.minor` in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) (e.g. `0.2.0` → releases become `v0.2.N`).
- Commits that only touch Markdown/docs don't trigger a release. Add `[skip ci]` to a commit message to skip one manually.
- Pull requests are checked by the [CI workflow](.github/workflows/ci.yml); [Dependabot](.github/dependabot.yml) keeps dependencies up to date.
- Update signing uses the `TAURI_SIGNING_PRIVATE_KEY` repository secret. Without it, releases still publish — just without auto-update files.

## 📄 License

[MIT](LICENSE) — free to use, modify and share.

---

<div align="center">
<sub>Made for people who solve the same problem twice and wish they'd written it down the first time.</sub>
</div>

<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="112" alt="BrainTracker logo" />

# BrainTracker

**Remember what you did — and exactly how you did it.**

A tiny, fast, offline-first Windows app for logging your work: the fix, the command,<br/>
the setting you changed, the screenshot that explains it. Find any of it again in seconds.

[![Latest release](https://img.shields.io/github/v/release/ioannismil/braintracker?style=flat-square&color=d9603b&label=latest)](https://github.com/ioannismil/braintracker/releases/latest)
[![Release build](https://img.shields.io/github/actions/workflow/status/ioannismil/braintracker/release.yml?branch=main&style=flat-square&label=build)](https://github.com/ioannismil/braintracker/actions/workflows/release.yml)
[![Downloads](https://img.shields.io/github/downloads/ioannismil/braintracker/total?style=flat-square&color=f2a65a)](https://github.com/ioannismil/braintracker/releases)
![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-2b2420?style=flat-square)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24c8db?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![License: MIT](https://img.shields.io/badge/license-MIT-6b5f56?style=flat-square)](LICENSE)

[**Download**](#-download) · [Features](#-features) · [Shortcuts](#%EF%B8%8F-keyboard-shortcuts) · [Your data](#-your-data) · [Build from source](#%EF%B8%8F-build-from-source)

</div>

---

## ✨ Features

<table>
<tr>
<td width="50%" valign="top">

### ⚡ Capture in seconds
Press <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>E</kbd> anywhere — a small popup appears on top of whatever you're doing. Type what you did, hit <kbd>Enter</kbd>, done. Click away and your half-written note is kept.

</td>
<td width="50%" valign="top">

### 🔎 Find anything instantly
Full-text search across titles, notes and tags, as you type, with highlighted matches. Partial words match, so `dock` finds *Docker*. Filter by tag with one click.

</td>
</tr>
<tr>
<td valign="top">

### 📝 Notes that hold the "how"
Markdown with code blocks, lists and links. Paste screenshots straight in with <kbd>Ctrl</kbd> <kbd>V</kbd>. Everything auto-saves as you type.

</td>
<td valign="top">

### 🗂️ Easy to navigate
Entries grouped by *Today · Yesterday · This week · month*. Pin the important ones to the top. Browse with the arrow keys without touching the mouse.

</td>
</tr>
<tr>
<td valign="top">

### 🖥️ Always there, never in the way
Starts with Windows and lives in the system tray. Summon the full window with <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd>. Closing just hides it.

</td>
<td valign="top">

### 🔒 Yours, and offline
No account, no cloud, no telemetry. One local SQLite file you can export to plain Markdown anytime. Every shortcut is configurable.

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
| New entry | <kbd>Ctrl</kbd> <kbd>T</kbd> | in the app |
| Search | <kbd>Ctrl</kbd> <kbd>K</kbd> | in the app |
| Toggle Markdown preview | <kbd>Ctrl</kbd> <kbd>E</kbd> | in the app |
| Settings | <kbd>Ctrl</kbd> <kbd>,</kbd> | in the app |

All of these can be changed in **Settings → Keyboard shortcuts** — click one, press the new keys. Shortcuts follow the physical key, so they work on any keyboard layout.

<details>
<summary><b>More keys</b></summary>

| Where | Keys |
|---|---|
| Search box | <kbd>↑</kbd> <kbd>↓</kbd> browse results · <kbd>Enter</kbd> open · <kbd>Enter</kbd> with no results creates an entry with that title · <kbd>Esc</kbd> clear |
| Quick capture | <kbd>Enter</kbd> save · <kbd>Shift</kbd>+<kbd>Enter</kbd> add notes · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Enter</kbd> save & open · <kbd>Ctrl</kbd>+<kbd>O</kbd> open app · <kbd>Esc</kbd> hide |
| Editor | <kbd>Tab</kbd> indents · <kbd>Ctrl</kbd>+<kbd>V</kbd> pastes screenshots · double-click the preview to edit |

</details>

## 💾 Your data

Everything stays on your machine.

| | Location |
|---|---|
| Installed | `%APPDATA%\com.braintracker.app\` |
| Portable | `data\` next to `BrainTracker.exe` |

Inside you'll find `braintracker.db` (all entries) and `images\` (pasted screenshots).

**Backups** run automatically once a day when something changed, into `Documents\BrainTracker Backups` (portable: `data\backups`). Change the folder, how many to keep, or restore one in **Settings → Automatic backups**. A restore always saves a *before-restore* snapshot of your current data first, and keeps your current settings.

**Export:** **Settings → Export** writes every entry as a Markdown file — screenshots included — to `Documents\BrainTracker Export`.

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

<details>
<summary><b>Project structure</b></summary>

```
braintracker/
├─ src/                      React + TypeScript UI
│  ├─ App.tsx                main window: sidebar, list, editor
│  ├─ Capture.tsx            quick-capture popup
│  ├─ Settings.tsx           settings dialog & shortcut recorder
│  ├─ shortcuts.ts           layout-independent shortcut matching
│  └─ images.ts              screenshot paste / rendering
├─ src-tauri/                Rust backend (Tauri 2)
│  ├─ src/db.rs              SQLite schema + FTS5 full-text search
│  ├─ src/lib.rs             commands, tray, global shortcuts, autostart, portable mode
│  └─ tauri.conf.json        windows, bundle & version (major.minor)
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

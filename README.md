# opencode runner

**English** | [繁體中文](README.zh-TW.md)

![opencode runner](docs/screenshot.png)

A lightweight desktop app for **scheduling and running [opencode](https://opencode.ai) tasks** — cron jobs, one-shot runs, or manual triggers — with a clean UI for editing prompts, watching live output, browsing run history, and reading the resulting agent conversations.

Point it at a project directory, write a prompt, give it a schedule, and let it run your AI coding agent unattended. A daily code review, a nightly dependency check, a scheduled changelog draft — anything you'd run `opencode run` for, now on a timer and with a record of every run.

Built with [Tauri 2](https://tauri.app) (Rust backend + React/TypeScript frontend). Ships as a small native app for Windows and Linux that lives in your system tray.

---

## Features

- **Flexible scheduling** — cron expressions (Quartz 6-field), one-shot runs at a specific time, or manual "Run now" triggers.
- **Per-task configuration** — working directory, prompt, model, and an optional hard timeout, all editable in the app.
- **Live run output** — tail opencode's stdout/stderr in real time as a run executes, plus a step-by-step event timeline.
- **Run history** — every run is recorded in a local SQLite database with status, timing, the exact prompt sent, and captured logs. Configure a per-task retention cap to keep history bounded.
- **Conversation viewer** — read the full agent conversation for any run, pulled directly from opencode's on-disk session data.
- **Run comments** — leave notes on individual runs.
- **Task memory** *(opt-in)* — let a task accumulate memory across runs. Its saved memory and your recent comments are folded into the prompt, and the agent can update that memory mid-run through a task-scoped MCP server.
- **Git worktree isolation** *(opt-in)* — run a task in a throwaway, detached git worktree so repeated or parallel runs never mutate your working checkout. Optionally base each worktree on a fresh `origin/main` (or any ref).
- **System tray** — closing the window hides to the tray; the app keeps running schedules in the background. Quitting from the tray shuts down gracefully, cancelling in-flight runs and cleaning up worktrees.
- **Auto-update** — signed updater artifacts; the app checks for and offers new releases.
- **Bilingual UI** — English and Traditional Chinese (繁體中文), with light and dark themes.

---

## Requirements

- **[opencode](https://opencode.ai)** installed and available on your `PATH` (or point the app at the binary explicitly in Settings). The app shells out to `opencode run` for every task and reads opencode's local session database to show conversations.
- A configured opencode setup (provider credentials, default model, etc.). The app uses your existing opencode configuration; if a task leaves the model blank, opencode's own default applies.

---

## Installation

Download the latest installer from the [Releases](https://github.com/BingHanLin/opencode-runner/releases) page:

- **Windows** — `opencode runner_<version>_x64-setup.exe` (NSIS installer; fetches the WebView2 runtime on first run if it's missing).
- **Linux (Debian / Ubuntu 22.04+)** — `opencode-runner_<version>_amd64.deb`:
  ```sh
  sudo apt install ./opencode-runner_*.deb
  ```
- **Linux (any distro, portable)** — `opencode_runner_<version>_amd64.AppImage`:
  ```sh
  chmod +x opencode_runner_*.AppImage
  ./opencode_runner_*.AppImage
  ```

---

## Getting started

1. **Launch the app.** On first run it has no tasks.
2. **Check Settings.** If `opencode` isn't on your `PATH`, set the path to the binary explicitly. (Leaving it unset falls back to a `PATH` lookup, which is convenient but vulnerable to PATH hijacking — production setups should set it.) You can also set a global cap on how many finished runs to retain per task.
3. **Create a task** with the **+** button and fill in:
   - **Name** — how you'll identify the task.
   - **Working directory** — the project opencode runs against.
   - **Prompt** — what the agent should do.
   - **Schedule** — cron, one-shot, or manual (see below).
   - **Model** *(optional)* — leave blank to use opencode's default.
   - Optional toggles: timeout, skip-permissions, worktree isolation, task memory.
4. **Save.** The scheduler picks the task up immediately.
5. **Run it.** Hit **Run now** to trigger on demand, or wait for the schedule. Switch to the **History** tab to watch live output, browse past runs, and read conversations.

---

## Scheduling

Each task has one schedule, set in the editor (and stored in `tasks.toml`):

| Type | Format | Example |
|------|--------|---------|
| **Cron** | `cron:<Quartz expression>` | `cron:0 0 9 ? * MON-FRI` |
| **One-shot** | `once:<RFC3339 timestamp>` | `once:2026-05-28T09:00:00Z` |
| **Manual** | `manual` | only fires on **Run now** |

> **Note on cron:** schedules use **Quartz 6-field** syntax — `second minute hour day-of-month month day-of-week` (with an optional 7th field for year), **not** the traditional Unix 5-field format. The day-of-month and day-of-week fields can't both be specific; set the unused one to `?`.

More examples:

```
cron:0 0 9 ? * MON-FRI    # weekdays at 09:00:00
cron:0 */15 * ? * *       # every 15 minutes
cron:0 0 0 1 * ?          # midnight on the 1st of every month
```

The app shows a plain-language description of each cron expression as you type it.

---

## How it works

```
┌─────────────────────────────┐
│  opencode runner             │
│                              │
│  React UI ── Tauri commands ─┼──► Scheduler ──► Runner ──► `opencode run`
│                              │        │            │
│                              │        │            ├─► SQLite run history
│                              │        │            └─► (optional) git worktree
│                              │        │
│                              │        └─► cron / once / manual triggers
└─────────────────────────────┘
                                          reads ◄── opencode's session DB
```

- Tasks live in a `tasks.toml` file in the app's per-user data directory.
- The **scheduler** registers each enabled task and fires the runner on cron/one-shot triggers.
- The **runner** invokes `opencode run --dir <working_dir> --format json …`, streams its output to the UI and a local SQLite database, and records the run's lifecycle. Only one run per task executes at a time.
- The **conversation viewer** reads opencode's own session database (read-only) to display the agent's messages.

### Data locations

The app stores its config and history in the standard per-user app data directory, resolved from the bundle identifier `dev.opencode.runner`:

| OS | Path |
|----|------|
| Windows | `%APPDATA%\dev.opencode.runner\` |
| macOS | `~/Library/Application Support/dev.opencode.runner/` |
| Linux | `~/.local/share/dev.opencode.runner/` |

That directory holds `tasks.toml` (your tasks and settings) and `runs.db` (run history, logs, events, comments, and per-task memory).

### `tasks.toml` example

You normally edit tasks in the UI, but the file is plain TOML. See [`tasks.example.toml`](./tasks.example.toml):

```toml
[[task]]
id = "daily-codereview"
name = "Daily code review"
schedule = "cron:0 0 9 ? * MON-FRI"
working_dir = "D:/projects/foo"
# model = "anthropic/claude-sonnet-4-6"   # blank → opencode's default
prompt = """
Review `git diff HEAD~1` in the working directory and list potential
issues with suggested fixes as bullet points.
"""
dangerously_skip_permissions = true
enabled = true
```

---

## Advanced features

### Git worktree isolation

When a task has **Run in worktree** enabled and its working directory is a git repo, the runner creates a detached, throwaway worktree, runs opencode there, and removes it afterward — so unattended runs never touch your live checkout. You can set a **worktree base** (e.g. `origin/main`); the runner does a `git fetch --all` first, verifies the ref, and forks the worktree from it. Files git-ignores can be carried into the worktree by listing them in a `.worktreeinclude` file at the repo root (one path per line).

### Task memory

When **memory** is enabled, each run receives the task's accumulated memory plus your recent run comments, woven into the prompt. A task-scoped MCP server is wired in for the run, exposing `runmem_*` tools the agent uses to read, append to, or rewrite its memory before finishing. Memory is per-task and stored in the local database — it's how a recurring task can learn from its own past runs.

---

## Building from source

### Prerequisites

- [Node.js](https://nodejs.org) 20+
- [Rust](https://rustup.rs) (stable)
- Platform Tauri 2 prerequisites — see the [Tauri setup guide](https://tauri.app/start/prerequisites/). On Debian/Ubuntu:
  ```sh
  sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
  ```

### Develop

```sh
npm install
npm run tauri dev
```

### Build a release bundle

```sh
npm run tauri build
```

Installers are produced under `target/release/bundle/`.

### Releasing

Pushing a `v*` tag (e.g. `git tag v0.6.0 && git push --tags`) triggers the [release workflow](./.github/workflows/release.yml), which builds Windows and Linux installers and attaches them to a draft GitHub Release. The pushed tag is the single source of truth for the version.

---

## Tech stack

- **[Tauri 2](https://tauri.app)** — native shell and Rust backend
- **Rust** — scheduler, runner, SQLite (`rusqlite`) persistence, cron parsing, MCP memory server
- **React 18 + TypeScript** — frontend
- **[Vite](https://vitejs.dev)** — build tooling

---

## License

Released under the [MIT License](./LICENSE).

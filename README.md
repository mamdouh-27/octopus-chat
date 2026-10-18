# Octopus Chat

A local-first AI agent desktop app. It reads and edits real files in your
projects, runs commands, and asks before it changes anything.

Built with Tauri v2 + Rust and React 19 + TypeScript. The Windows installer is
**3.4 MB** and the running app sits at roughly **30 MB of RAM**, because it uses
the operating system's webview instead of bundling a browser.

## What works today

**Chat and agent**
- Streaming responses with live markdown, GitHub-flavoured tables and
  syntax-highlighted code
- Plan / Build mode. Plan mode is structurally read-only: no tool in the
  registry can mutate the workspace, so the guarantee does not depend on a
  setting
- Tool loop with `read`, `list`, `glob`, `grep`, `write`, `edit`, `delete`,
  `bash` and a task list
- Tool results are streamed into the transcript with expandable cards and
  unified diffs
- Reasoning traces are shown collapsed, when the provider sends them
- A message queue: keep typing while the agent works
- Stop mid-run; the partial answer is kept and marked as stopped

**Safety**
- Three permission modes: ask for approval, approve for me, full access
- Allow/deny rules per project, matched on `tool` or `tool:argument`
- A short list of commands that can never be approved (`rm -rf /`, `mkfs`, …)
- Every path is resolved inside the project roots; `../` escapes are refused
- A git checkpoint is taken before each turn, so fork & rewind can restore the
  working tree, including your own uncommitted work

**Models**
- OpenAI, Anthropic and Gemini wire formats, implemented once each
- Local providers with no key and no outbound traffic: Ollama, LM Studio,
  llama.cpp
- Cloud: OpenRouter, Groq, Mistral, DeepSeek, xAI, Together, Fireworks,
  GitHub Models
- **Add any provider**: a custom base URL and wire format is enough
- **Add any model**: type the id your endpoint uses, including models released
  after this build, or let Octopus ask the endpoint what it serves
- Test a provider's reachability before trusting it with a key

**Interface**
- 34 bundled themes across dark and light, including three true-black variants
- Code blocks follow the active theme, because the highlighter is driven by the
  same tokens as the rest of the UI
- Command palette (`mod+k`) searching chats, projects, themes and commands
- Remappable keyboard shortcuts
- Git branch and changed-file count in the side panel's Changes tab
- Bidirectional text: prose renders with `dir="auto"`, so an Arabic or Hebrew
  reply reads right-to-left while embedded code stays left-to-right. The
  composer's direction button cycles auto / LTR / RTL when one has to be forced
- Click any attached image to open a full-size viewer, with arrow-key stepping
  through a multi-image message
- Reasoning effort (Auto / Low / Medium / High), shown only for models that
  support it, persisted per chat and sent as `reasoning_effort`

## Running it

```bash
npm install
npm run tauri:dev          # development
npm run tauri:build        # installer -> src-tauri/target/release/bundle
npm test                   # typecheck + 102 Rust tests
```

### Point it at a model

Ollama is the quickest start:

```bash
ollama serve
ollama pull qwen3-coder:30b
```

Then open **Settings → Providers → On this machine**, press the wifi button
next to Ollama to fetch its model list, and pick one in the composer.

For a cloud provider, paste the key in **Settings → Providers → API keys**. Keys
are stored in the app database on this machine and are never sent anywhere else.

### Projects

A project is one or more folders plus optional instructions. An `AGENTS.md` in
the workspace root is read on every request automatically.

## Layout

```
src/                    React frontend
  lib/ipc.ts            typed wrappers over the Rust command surface
  store/chat.ts         application state and the streaming event bridge
  theme/                token contract, 34 palettes, CSS-variable engine
  styles/               four stylesheets, every colour a theme token
src-tauri/
  core/                 octopus-core: storage, providers, tools, agent. No Tauri.
  src/                  the Tauri shell: commands, events, window
```

The split matters: `octopus-core` has no webview dependency, so its tests run
without loading one. The integration tests in `core/tests/agent_loop.rs` drive
the whole agent loop against a mock OpenAI-compatible server over real HTTP.

### The IPC naming contract

Every struct in `core/src/models.rs` crosses the Tauri boundary as a whole
object, and every one of them carries
`#[serde(rename_all = "camelCase")]`. The frontend is TypeScript and sends
camelCase (`permissionMode`, `tokensIn`); without that rename Serde expects the
snake_case Rust spelling and rejects the entire payload with
`missing field ...`, which the UI can only report as a generic failure.

This is not hypothetical. `models.rs` was the one file in the crate missing the
attribute, so `project_save`, `session_update`, `todos_save` and
`provider_save_custom` had all been silently failing — creating a project simply
never worked. `core/tests/ipc_contract.rs` now pins the exact camelCase payloads
so a dropped attribute fails a test instead of a feature.

`cargo test -p octopus-core` is what `npm test` runs. It covers the core crate
only, because building the Tauri crate in test mode needs the whole dependency
graph and does not fit in memory on machines with limited RAM. See
[Troubleshooting](#troubleshooting-low-disk-space-or-memory-on-windows).

### Storage notes

Migrations are forward-only in `core/src/db/schema.rs`; each entry moves
`PRAGMA user_version` forward by one and a shipped entry is never edited.

Two of them exist because of bugs that were invisible from the UI:

- **v3** repairs project roots stored in the Windows extended-length form. Older
  versions saved roots exactly as `Path::canonicalize` returned them, which
  carries a `\\?\` prefix. The sandbox compares roots against ordinary `C:\...`
  tool paths, so an affected project denied every read and write. The repair
  parses and re-encodes the JSON in Rust rather than using SQL `replace()`,
  because JSON escapes every backslash and a textual substitution matches at a
  misaligned offset and corrupts the path.
- **v4** rebuilds `custom_models` without the foreign key on `provider_id`. The
  built-in providers (Ollama, LM Studio, an OpenAI-compatible endpoint) are
  catalog entries with no row in `custom_providers`, so adding a model to one of
  them failed outright with `FOREIGN KEY constraint failed` — "Add model" only
  ever worked for providers the user had created themselves. The new primary key
  is `(provider_id, id)`, because the same model id under two providers is
  legitimate.

### How a theme works

A theme declares about 25 colours. `buildTokens` expands that into the full
semantic token set, deriving the long tail as opaque steps in the theme's own
OKLCH space. Every resolved value is therefore a solid colour rather than a
translucent `color-mix` expression, which keeps hover and selected states
predictable against the surface they sit on. Applying a theme writes CSS custom
properties onto `:root`, so switching themes costs one style recalculation and
zero React renders.

Code highlighting uses a Shiki theme whose colours are `var(--oc-*)` rather
than fixed values, which is why code blocks follow whatever theme is active.
Grammars load on demand, so a transcript that only contains TypeScript never
downloads the Rust grammar.

## Performance notes

- No Electron: the window uses WebView2 on Windows and WebKitGTK on Linux
- `panic = "abort"`, LTO and `opt-level = "s"` in release
- Syntax grammars are separate lazy chunks, fetched only when used
- The JavaScript regex engine is used instead of the Oniguruma WASM, saving
  roughly 600 KB and a WebAssembly instantiation
- Sends stream through a channel rather than being buffered

## Build environment (optional)

`build-env.json` is OPTIONAL and git-ignored. On a machine with a normal
system drive, do not create it: `scripts/run.mjs` and `env.ps1` fall back to
the unmodified process environment, and the standard prerequisites are enough.
See `build-env.example.json` for the documented optional fields (`buildRoot`,
`linker`, `buildJobs`, `extraPath`).

On machines where the system drive is full, `build-env.json` points the Rust
toolchain, crate registry and build output at a build root outside the app
source, such as `D:\octopus-chat-build`. The `linker` field sets
`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER`; the tracked
`src-tauri/.cargo/config.toml` carries only the portable `[net]
git-fetch-with-cli = true`, so a fresh clone builds without machine paths.

The Windows Rust target is `x86_64-pc-windows-gnu` using MSYS2 UCRT64 as the
linker, so no Visual Studio C++ workload is required. `src-tauri/build.rs`
stages `WebView2Loader.dll` next to the binaries, which cargo does not do for
dev and test targets on that target.

## Troubleshooting: low disk space or memory on Windows

Two failure modes are worth recognising before changing anything:

- **Do not touch `[profile.release]`.** Editing it changes the profile hash,
  which invalidates the cached build of every dependency, including the very
  large `windows` crate. A cold rebuild of that does not fit in memory on a
  machine with limited RAM.
- **Free space on the system drive before building.** The pagefile lives there.
  At zero bytes free it cannot grow, and rustc fails with `os error 1455`
  ("paging file is too small") while merely mapping its own standard library.
  Pointing `RUSTUP_HOME`, `CARGO_HOME` and `CARGO_TARGET_DIR` at another drive
  via `build-env.json` keeps that drive free.

## Safety notes

- The app reads and writes files on your machine. Permission mode is the main
  control; treat "Full access" as equivalent to running the model yourself.
- API keys (built-in and custom providers) live unencrypted in
  `%APPDATA%/octopus-chat/octopus.db`. That is deliberate for a local app,
  but it means anything that can read your user profile can read your keys.
- Rewind deletes untracked files in the project. The confirmation dialog says so.

## Not built yet

Honest list of what is stubbed or missing:

- Parallel agents, git worktree isolation and the agent manager
- Scheduled tasks, skills, MCP servers, plugins and hooks
- An integrated terminal
- Rule editing in the UI (rules load and are enforced, but there is no editor
  screen yet)
- Linux and macOS builds are unverified; the Rust code is cross-platform, the
  linker setup is not

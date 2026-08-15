# Repository Guidelines

## Project Overview
**OMP Model Studio** is a lightweight desktop control plane for [Oh My Pi (`omp`)](https://github.com/canis-aur/oh-my-pi). It provides a visual interface for managing and previewing:
- **Built-in & Custom Model Roles**: Configures `modelRoles.*` selectors (e.g. `@slow`, `@smol`, `@tester`) across configured providers and thinking efforts.
- **Built-in Agent Model Overrides**: Configures model selectors for bundled agents (`reviewer`, `security-reviewer`) under `task.agentModelOverrides.*`.
- **Custom Agent Definitions**: Visual authoring and editing for standalone agent definitions (`~/.omp/agent/agents/*.md`) with frontmatter metadata.
- **Provider Enable/Disable & Model Presets**: Controls active provider scopes and saves local role preset snapshots.

The app is built as a single-window Tauri v2 desktop application with a React 19 frontend and a Rust native backend.

---

## Architecture & Data Flow
```text
┌─────────────────────────────────────────────────────────┐
│ React 19 Frontend (src/App.tsx)                          │
│ - Draft state & validation (roles, agents, overrides)   │
│ - Local preset & theme storage (localStorage)           │
└───────────────────────────┬─────────────────────────────┘
                            │ Tauri IPC (`@tauri-apps/api/core`)
                            ▼
┌─────────────────────────────────────────────────────────┐
│ Tauri v2 / Rust Backend (src-tauri/src/lib.rs)          │
│ - IPC Commands: `load_snapshot`, `save_snapshot`        │
│ - Model Discovery: `omp models --json` -> models.yml    │
│ - Non-destructive YAML manipulation (serde_yaml)        │
│ - Atomic persistence (write tmp -> rename)              │
└───────────────────────────┬─────────────────────────────┘
                            │ Read / Atomic Write
                            ▼
┌─────────────────────────────────────────────────────────┐
│ User Filesystem Boundaries                              │
│ - Global Config: `~/.omp/agent/config.yml`               │
│ - Agent Definitions: `~/.omp/agent/agents/*.md`          │
│ - Model Catalog Cache: `~/.omp/agent/models.yml`        │
└─────────────────────────────────────────────────────────┘
```

1. **Frontend Layer (`src/`)**:
   - `App.tsx` runs the primary UI state machine. It handles slot cards, modal editors, validation rules, preset storage (`localStorage`), and theme switching (`data-theme`).
   - `api.ts` defines typed IPC invocations (`load_snapshot`, `save_snapshot`).
   - `types.ts` maintains TypeScript interfaces matching Rust IPC payloads.
2. **Backend Layer (`src-tauri/src/lib.rs`)**:
   - Locates user OMP paths via environment variables (`HOME` / `USERPROFILE`).
   - `load_snapshot` queries installed models by running `omp models --json`, parsing `~/.omp/agent/models.yml`, reading `config.yml`, and loading markdown agent definitions from `~/.omp/agent/agents/`.
   - `save_snapshot` validates inputs, updates only target YAML mappings while preserving unrecognized keys and comments, writes standalone agent markdown files, and uses temporary files (`.<name>.omp-model-studio.tmp`) with atomic rename replacement.

---

## Key Directories
- `src/`: React 19 frontend source code (components, styles, IPC bridge, type definitions).
- `src-tauri/`: Rust backend crate (`app_lib`), Cargo configuration, build hooks, and capabilities.
- `src-tauri/src/`: Rust entry points (`main.rs`, `lib.rs`).
- `src-tauri/capabilities/`: Tauri v2 desktop security and capability configurations (`default.json`).
- `src-tauri/icons/`: Application icon assets for macOS, Windows, and Linux packaging.
- `dist/`: Generated frontend static build output (gitignored).

---

## Development Commands

```bash
# Install frontend dependencies
bun install
# or: npm install

# Start development desktop application (Vite dev server + Tauri dev host)
bun run tauri dev
# or: npm run tauri dev

# Run Vite dev server standalone (port 1420)
bun run dev
# or: npm run dev

# Typecheck and build frontend distribution
bun run build
# or: npm run build

# Preview built frontend distribution
bun run preview
# or: npm run preview

# Run backend Rust unit tests
cargo test --manifest-path src-tauri/Cargo.toml
# or: cd src-tauri && cargo test

# Build production desktop release package
bun run tauri build
# or: npm run tauri build
```

---

## Code Conventions & Common Patterns

### 1. Separation of Role vs. Agent
- **Model Role** (`modelRoles.<role>`): Defines a model selector string (e.g. `openai-codex/gpt-5.6-terra:max`). It controls model choice, not execution permissions or tasks.
- **Built-in Agent Model Override** (`task.agentModelOverrides.<agent>`): Overrides the model used by a bundled agent (`reviewer`, `security-reviewer`).
- **Custom Agent** (`~/.omp/agent/agents/<name>.md`): Markdown file with YAML frontmatter defining name, description, model selector (e.g. `@tester`), and prompt body.

### 2. Frontend State & Validation Patterns
- **No Direct Native Popups**: Avoid `window.confirm` or `window.alert` inside WebView; use two-step inline action confirmations (e.g. `Delete` -> `确认删除`).
- **Non-Destructive UI Drafts**: UI edits stay in draft state until the user explicitly clicks `Apply` (writing to disk via Rust backend).
- **Selector Format**: Models follow `provider/model-id[:effort]` (e.g., `google-antigravity/gemini-3.7-flash:high`) or aliases (`@slow`, `@task`).
- **CSS Architecture**: Single vanilla stylesheet `src/styles.css` utilizing CSS variables, `color-mix`, and semantic tokens (`--bg`, `--panel`, `--border`, `--accent`).

### 3. Rust Backend Conventions
- **Non-Destructive Config Mutation**: `update_yaml_config()` parses `config.yml` into a dynamic `serde_yaml::Value::Mapping` to update only managed sections (`modelRoles`, `task.agentModelOverrides`, `disabledProviders`), preserving unmanaged user keys and formatting.
- **Atomic File Writing**: Files must be written to sibling hidden temporary files and renamed (`fs::rename`) to prevent partial writes during crashes or interruptions.
- **Path Sanitization**: Backend helper `display_path()` replaces user home paths with `~/` before returning data across the IPC boundary to avoid leaking local usernames.

---

## Important Files
- `src/App.tsx`: Main React desktop interface, state transitions, modals, and preset controls.
- `src/types.ts`: TypeScript contracts for `ConfigSnapshot`, `ModelOption`, and `SaveRequest`.
- `src/api.ts`: Tauri IPC client bindings.
- `src/styles.css`: Visual design system and theme styles.
- `src-tauri/src/lib.rs`: Rust backend core logic (YAML parsing, model discovery, validation, IPC commands, unit tests).
- `src-tauri/src/main.rs`: Desktop binary runner.
- `src-tauri/tauri.conf.json`: Tauri v2 manifest, window sizing (1280x900, min 1040x700), build hooks.
- `src-tauri/capabilities/default.json`: Tauri permissions definition (`core:default`).
- `package.json` & `tsconfig.json`: Frontend build and compiler specifications.
- `src-tauri/Cargo.toml`: Backend Rust dependency definitions.

---

## Runtime & Tooling Preferences
- **Frontend Package Manager**: `bun` is preferred (`tauri.conf.json` invokes `bun run dev` / `bun run build`), with `npm` as a compatible fallback.
- **Frontend Framework**: React 19, TypeScript 5 (Strict Mode, `noEmit: true`), Vite 7.
- **Backend Toolchain**: Rust 2021 Edition with MSRV `1.77.2`.
- **Desktop Framework**: Tauri v2 (`tauri` `2.11.3`, `@tauri-apps/cli` `^2.11.4`).
- **Environment**: Node/Bun on Darwin / macOS (also supports Windows/Linux through standard Tauri toolchains).

---

## Testing & QA
- **Backend Tests**: Rust unit tests are located in `src-tauri/src/lib.rs` under `#[cfg(test)] mod tests`.
  - Tests verify non-destructive YAML modification, custom role addition, override preservation, disabled provider updates, and agent file generation.
  - Tests use temporary directories (`tempfile` / `env::temp_dir()`) with isolated `HOME` overrides.
  - Run with: `cargo test --manifest-path src-tauri/Cargo.toml`.
- **Frontend Verification**: TypeScript typecheck and bundle build validation via `bun run build` (`tsc --noEmit && vite build`).

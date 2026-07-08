# Agent Instructions

This repository uses Superpowers as the preferred agent workflow layer.

Superpowers is not a runtime dependency of the Tauri/React/Rust app. Install it in
your coding agent, then use its skills while working in this repo.

## Superpowers

- Source: https://github.com/obra/superpowers
- Codex plugin name: `superpowers`
- Observed plugin version: `6.1.1`
- Install in Codex App or Codex CLI from the official plugin marketplace by
  opening `/plugins`, searching for `Superpowers`, and selecting `Install Plugin`.

## Project Workflow

Use the relevant Superpowers skill before starting substantial work:

- `brainstorming` for unclear product or architecture changes.
- `writing-plans` before multi-file implementation work.
- `test-driven-development` for behavior changes where tests are practical.
- `systematic-debugging` for regressions, flaky behavior, or root-cause work.
- `requesting-code-review` before considering a larger change complete.
- `verification-before-completion` before final handoff.

For small mechanical fixes, keep the change focused and verify with the nearest
available command, usually `npm run typecheck`, `npm run lint`, `npm run test`, or
the relevant Cargo command under `src-tauri`.

## Repository Notes

- Frontend: React, TypeScript, Vite, Tailwind.
- Desktop backend: Tauri 2 with Rust.
- Treat `appTiktok/`, `release/`, `dist/`, `node_modules/`, and
  `src-tauri/target/` as generated or local-only assets.
- Do not revert unrelated user changes in this worktree.

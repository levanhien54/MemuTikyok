# Superpowers Integration

This project integrates `obra/superpowers` as an agent workflow dependency, not
as an application runtime dependency.

Superpowers provides coding-agent skills for planning, TDD, debugging, review,
and delivery. It should be installed in the developer's agent environment and
used while editing this repository.

## Install For Codex

Superpowers is available in the official Codex plugin marketplace.

1. Open the Codex plugin interface with `/plugins`.
2. Search for `Superpowers`.
3. Select `Install Plugin`.
4. Restart or start a new Codex session in this repository.

The upstream repository is https://github.com/obra/superpowers.

At the time of integration, the upstream Codex plugin manifest reported:

```json
{
  "name": "superpowers",
  "version": "6.1.1",
  "repository": "https://github.com/obra/superpowers"
}
```

## How It Is Wired Into This Repo

The root `AGENTS.md` tells coding agents that this project expects Superpowers
workflows for non-trivial changes. This keeps the application source clean while
making the workflow visible to every agent session.

Use these Superpowers skills as the default path:

- `brainstorming` when the request is still ambiguous.
- `writing-plans` for multi-step or multi-file work.
- `test-driven-development` for behavior changes where tests can be written.
- `systematic-debugging` for bugs and regressions.
- `requesting-code-review` before completing larger changes.
- `verification-before-completion` before final handoff.

## Why Not Vendor The Repository?

Vendoring `obra/superpowers` into this app would mix agent tooling with product
code and make updates harder. Installing it through the Codex plugin marketplace
keeps Superpowers updateable and avoids shipping agent-only files in the Tauri
application.

If offline or pinned operation becomes necessary later, add the upstream repo as
a Git submodule under a tooling-only path such as `tools/superpowers/`, then keep
that path out of application builds.

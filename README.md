# coo

Personal, single-operator AI companion application — native desktop, local-first, multi-year horizon. The centerpiece is the operator-facing AI named **Exile**, defined permanently in `doctrine/EXILE.md`.

COO is an independent product in the workspace per [ADR-0011](../doctrine/decisions/0011-coo-as-independent-product.md), peer to `dobackbone/`, `fom/`, and `command-flow/`. It consumes the shared workspace doctrine in `../doctrine/` and ships its own product-local agent doctrine in `doctrine/` (this repo).

## Status

**Phase 0 — Doctrine and design.** Implementation has not begun. The MVP scope contract is in [`../doctrine/mvp/coo.md`](../doctrine/mvp/coo.md).

| Phase 0 item | Status |
|---|---|
| `doctrine/EXILE.md` | v0.3 complete |
| `doctrine/RAPPORT-STATE-MODEL.md` | v0.2 complete |
| `../doctrine/decisions/0011-coo-as-independent-product.md` | Accepted |
| `../doctrine/mvp/coo.md` | Committed (doctrine c170f73) |
| Tauri vs. Electron | Decided — Tauri |
| AI runtime | Decided — Anthropic Claude API + abstraction |
| Exile character art generation | Pending — own session |

When the character art pass closes, Phase 0 is complete and Phase 1 (MVP build) begins per `../doctrine/mvp/coo.md`.

## Reading order for a fresh session

If you are landing in this repository — a new Claude Code session, a different model, a human collaborator — read in this order:

1. **`CLAUDE.md`** (this repo) — Claude Code standing instructions for COO. Names what's load-bearing here.
2. **`doctrine/EXILE.md`** — the character. Sections 1 and 1.5 are the operator's verbatim writing and are permanent at finalization.
3. **`doctrine/RAPPORT-STATE-MODEL.md`** — the schema and behavior spec for Exile's state across sessions.
4. **`../doctrine/mvp/coo.md`** — the MVP scope contract.
5. **`../doctrine/decisions/0011-coo-as-independent-product.md`** — the ADR establishing COO's independence in the workspace.
6. **`../doctrine/CLAUDE.md`** — ecosystem-wide Claude Code instructions; binding above this repo's `CLAUDE.md`.
7. **`../doctrine/handoffs/coo-phase-0-handoff.md`** — historical snapshot of Phase 0 design state.

`EXILE.md` is the centerpiece. The application is, in the operator's words, *"a representation of the agent."* Read her first.

## Stack (decided as of Phase 0)

- **Tauri** for the desktop application shell
- **Rust** backend (encryption, SQLite, inference abstraction client)
- **Local SQLite** under `~/.coo/`
- **Encrypted at rest** — per-domain encryption with operator-derived master key
- **Anthropic Claude API** for inference at MVP, behind an abstraction layer enabling provider swap
- **No Docker.** Native desktop application.

Frontend framework choice deferred to Phase 1 entry. Not gated by doctrine.

## What this product is *not*

- Not a multi-user product
- Not a marketable SaaS
- Not a productivity suite (briefs and kit serve Exile's role as handler; they are not standalone productivity features)
- Not federated operator-to-operator
- Not a fork of dobackbone

Per ADR-0011, COO is the operator's personal companion application. It connects to dobackbone instances owned by the same operator via the BACKBONE-CONTRACT when present, but does not require one to function.

# coo

Personal, single-operator AI companion application — native desktop, local-first, multi-year horizon. The centerpiece is the operator-facing AI named **Exile**, defined permanently in `doctrine/EXILE.md`.

COO is an independent product in the workspace per [ADR-0011](../doctrine/decisions/0011-coo-as-independent-product.md), peer to `dobackbone/`, `fom/`, and `command-flow/`. It consumes the shared workspace doctrine in `../doctrine/` and ships its own product-local agent doctrine in `doctrine/` (this repo).

## Status

**Phase 1 — MVP build, in progress.** The MVP scope contract is in [`../doctrine/mvp/coo.md`](../doctrine/mvp/coo.md). Phase 1 was unblocked at 2026-04-28 while Phase 0's final item (Exile character art generation) remains operator-driven and asynchronous; engineering work proceeds in parallel.

| Phase 0 item | Status |
|---|---|
| `doctrine/EXILE.md` | v0.3 complete |
| `doctrine/RAPPORT-STATE-MODEL.md` | v0.2 complete |
| `../doctrine/decisions/0011-coo-as-independent-product.md` | Accepted |
| `../doctrine/mvp/coo.md` | Committed (doctrine c170f73) |
| Tauri vs. Electron | Decided — Tauri |
| AI runtime | Decided — Anthropic Claude API + abstraction |
| Exile character art generation | Pending — operator-driven |

| Phase 1 item | Status |
|---|---|
| §1 Tauri scaffolding + SQLite + migrations wired | ✅ Shipped 2026-04-28 |
| §2 Encrypted state at rest | ✅ Shipped 2026-04-29 — (a) Argon2id KDF + (b) HKDF derive + (c) XChaCha20-Poly1305 envelope |
| §3 Onboarding wizard | ✅ Shipped 2026-04-29 — (a) vault + (b) schema + (c) IPC + wizard/unlock + (d) startup gating with mid-crash wizard-resume |
| §4 Channel surface | Not started |
| §5 Inference abstraction layer | 🟡 (a) shipped 2026-04-30 — InferenceProvider trait + stub provider; (b) Claude impl pending |
| §6 State surfaces (Station, Dossier, Briefs, Kit, Calibration) | Not started |
| §7 Migration discipline | Precedent set in §1; applied in subsequent slices |

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

## Stack

- **Tauri 2** for the desktop application shell
- **Rust** backend (encryption, SQLite, inference abstraction client) — currently `rustc 1.95.0` MSVC
- **Frontend:** React + TypeScript + Vite (chosen at Phase 1 §1 entry, 2026-04-28; not gated by doctrine)
- **Local SQLite** under `~/.coo/coo.db` via `rusqlite` 0.32 (bundled, statically linked)
- **Migrations** via `rusqlite_migration` 1.x — append-only discipline per `doctrine/RAPPORT-STATE-MODEL.md` §7
- **Encrypted at rest** — per-domain encryption with operator-derived master key (Phase 1 §2, closed 2026-04-29). Argon2id KDF (m=65536, t=3, p=1) + HKDF-SHA256 per-domain derivation + XChaCha20-Poly1305 AEAD envelope (24-byte nonce, 6-byte header bound as AAD). §3 closed 2026-04-29 across four sub-slices: (a) vault layer with passphrase sentinel at `~/.coo/sentinel` + `vault::setup_passphrase`/`vault::unlock` state machine + `crypto::derive_lock_key` (HKDF info `coo/v1/lock`) sibling to the four state-domain derivations; (b) migration #2 with `app_config` (plaintext key/value), `operator_profile` (singleton-via-CHECK + encrypted BLOB), and `calibration_setting` (placeholder key/value, typed schema deferred to §6); (c1) Tauri IPC surface (managed `AppState` with vault + db, six commands); (c2) React wizard + unlock screen + routing; (d) startup gating with mid-crash wizard-resume routing. End-to-end verified: full wizard click-through wrote `salt` (34 B) + `sentinel` (62 B) + `coo.db`, and unlock-on-relaunch round-tripped correctly with both right and wrong passphrase.
- **Anthropic Claude API** for inference at MVP, behind an abstraction layer enabling provider swap (Phase 1 §5). §5 (a) shipped 2026-04-30: `InferenceProvider: Send + Sync` async trait + `StubProvider` (echo with `[stub]` prefix for token-free §4 development) + `build_provider()` constructor. Default model `claude-opus-4-7`, overridable via `COO_INFERENCE_MODEL`; key from `ANTHROPIC_API_KEY` (falls back to stub when unset). Claude implementation lands at §5 (b).
- **No Docker.** Native desktop application.

## What this product is *not*

- Not a multi-user product
- Not a marketable SaaS
- Not a productivity suite (briefs and kit serve Exile's role as handler; they are not standalone productivity features)
- Not federated operator-to-operator
- Not a fork of dobackbone

Per ADR-0011, COO is the operator's personal companion application. It connects to dobackbone instances owned by the same operator via the BACKBONE-CONTRACT when present, but does not require one to function.

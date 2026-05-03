# coo

Personal, single-operator AI companion application — native desktop, local-first, multi-year horizon. The centerpiece is the operator-facing AI named **Exile**, defined permanently in `doctrine/EXILE.md`.

COO is an independent product in the workspace per [ADR-0011 (The Personal-Commander Tier)](../doctrine/decisions/0011-personal-commander-tier.md), peer to `dobackbone/`, `fom/`, and `command-flow/`. It consumes the shared workspace doctrine in `../doctrine/` and ships its own product-local agent doctrine in `doctrine/` (this repo).

## Status

**Phase 1 — MVP build, in progress.** The MVP scope contract is in [`../doctrine/mvp/coo.md`](../doctrine/mvp/coo.md). Phase 1 was unblocked at 2026-04-28 while Phase 0's final item (Exile character art generation) remains operator-driven and asynchronous; engineering work proceeds in parallel.

| Phase 0 item | Status |
|---|---|
| `doctrine/EXILE.md` | v0.3 complete |
| `doctrine/RAPPORT-STATE-MODEL.md` | v0.5 complete |
| `../doctrine/decisions/0011-personal-commander-tier.md` | Accepted |
| `../doctrine/mvp/coo.md` | Committed (doctrine c170f73) |
| Tauri vs. Electron | Decided — Tauri |
| AI runtime | Decided — Anthropic Claude API + abstraction |
| Exile character art generation | Pending — operator-driven |

| Phase 1 item | Status |
|---|---|
| §1 Tauri scaffolding + SQLite + migrations wired | ✅ Shipped 2026-04-28 |
| §2 Encrypted state at rest | ✅ Shipped 2026-04-29 — (a) Argon2id KDF + (b) HKDF derive + (c) XChaCha20-Poly1305 envelope |
| §3 Onboarding wizard | ✅ Shipped 2026-04-29 — (a) vault + (b) schema + (c) IPC + wizard/unlock + (d) startup gating with mid-crash wizard-resume |
| §4 Channel surface | ✅ Shipped 2026-05-01 — (a) prompt + `infer` IPC + React `ChannelSurface` + (a3) output discipline + (b) conversation persistence + (c) in-character summarization |
| §5 Inference abstraction layer | ✅ Shipped 2026-04-30 — (a) InferenceProvider trait + stub provider, (b) Anthropic Claude impl |
| §6 State surfaces (Station, Dossier, Briefs, Kit, Calibration) | Not started |
| §7 Migration discipline | Precedent set in §1; applied in subsequent slices |

## Reading order for a fresh session

If you are landing in this repository — a new Claude Code session, a different model, a human collaborator — read in this order:

1. **`CLAUDE.md`** (this repo) — Claude Code standing instructions for COO. Names what's load-bearing here.
2. **`doctrine/EXILE.md`** — the character. Sections 1 and 1.5 are the operator's verbatim writing and are permanent at finalization.
3. **`doctrine/RAPPORT-STATE-MODEL.md`** — the schema and behavior spec for Exile's state across sessions.
4. **`../doctrine/mvp/coo.md`** — the MVP scope contract.
5. **`../doctrine/decisions/0011-personal-commander-tier.md`** — the ADR establishing the personal-commander tier and COO's place in it.
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
- **Anthropic Claude API** for inference at MVP, behind an abstraction layer enabling provider swap (Phase 1 §5, closed 2026-04-30). §5 (a): `InferenceProvider: Send + Sync` async trait + `StubProvider` (echo with `[stub]` prefix for token-free §4 development) + `build_provider()` constructor. §5 (b): `ClaudeProvider` against Anthropic's `/v1/messages` endpoint via `reqwest` (rustls-tls, json), hand-rolled serde over the Messages API surface (`{model, max_tokens: 4096, system, messages}`), `anthropic-version: 2023-06-01` pinned, content-block concatenation on success, error mapping (`401`/`403` → `Auth`, `429` → `RateLimited`, other → `Provider`, transport → `Network`), structured-error-envelope parsing with status-code fallback. Tests are hermetic via `wiremock`; no live API calls in CI. Default model `claude-opus-4-7`, overridable via `COO_INFERENCE_MODEL`; key from `ANTHROPIC_API_KEY` (falls back to the stub when unset or empty so the app remains launchable for UI-only work).
- **Channel surface (Phase 1 §4 closed 2026-05-01 across (a) + (a3) + (b) + (c)).** (a1) Rust IPC: new `prompt` module assembles the system prompt from `EXILE.md` §1 + §1.5 + §2 verbatim via `include_str!` + heading-marker slicing — the load-bearing core of `doctrine/RAPPORT-STATE-MODEL.md` §5.2's inference assembly pipeline. State-derived prose modifiers, calibration ceiling clamp, and conversation history context are deferred to subsequent slices because the data they read doesn't exist yet. `infer` Tauri command takes the conversation turn-list, assembles the system prompt server-side (JS never sees the doctrine text), and routes through `state.inference`. `InferenceCommandError` is a JSON-tagged enum so the React surface can pattern-match on `kind`. (a2) React UI: new `ChannelSurface` component with role-distinct scrollback bubbles, Cmd/Ctrl+Enter to send + plain Enter for newline, distinct error UI per `InferenceCommandError` variant + Retry button, conversation state in component-local `useState` (no persistence at §4 (a) — refresh = empty conversation, §4 (b) lands persistence). End-to-end at §4 (a) close: operator launches the app, unlocks the vault, lands in the Channel, types into the input, gets a real Anthropic Claude response (system prompt = `EXILE.md` §1 + §1.5 + §2 verbatim) when `ANTHROPIC_API_KEY` is set, or the stub echo otherwise. (a3) appends a Channel-surface output discipline directive to the system prompt per `doctrine/RAPPORT-STATE-MODEL.md` §5.5 — *in the Channel, you are a voice, not a scene* — so the bubble renders dialogue only, no italicized stage directions or third-person prose. Character text in `EXILE.md` §1 / §1.5 is unchanged; the discipline is a surface-level render directive on top. (b) makes conversation turns persistent: schema migration #3 adds `conversation_session` and `conversation_turn` (encrypted under `Domain::Conversation`); `INFERENCE_WINDOW_TURNS = 100` caps the in-window context (tier 1 of `doctrine/RAPPORT-STATE-MODEL.md` §4.1's three-tier retention model). The `infer` IPC contract changes to `infer(session_id, operator_turn) -> InferResponse`; disk is the source of truth, React state is a projection. New commands `load_conversation` / `append_turn`; new `ConversationCommandError` enum. React side: load-on-mount, optimistic operator-turn rendering with disk-authoritative replacement on success, "Today"/"Yesterday"/absolute date dividers between turns whose calendar day differs. (c) lands tiers 2 (within-session) and 3 (cross-session) of the retention model: schema migration #4 adds `conversation_summary` (encrypted) plus an additive `summarized_through_turn_index` column on `conversation_session`. `SESSION_INACTIVITY_GAP_HOURS = 6` and `SUMMARIZATION_BATCH_SIZE = 30` are the named constants. Within-session triggers when unsummarized turn count exceeds `INFERENCE_WINDOW_TURNS + SUMMARIZATION_BATCH_SIZE` (130); cross-session triggers on inactivity-gap detection inside `infer`. Both run synchronously inside `infer` — no background workers. Summaries are produced in Exile's own voice via a separate inference call (`prompt::SUMMARIZATION_DIRECTIVE` + `prompt::assemble_summarization_prompt`) and prepended to the operator-facing inference context as a synthetic assistant message — *Exile's own recollection*, not an external context block, per `doctrine/RAPPORT-STATE-MODEL.md` §4.2. `InferResponse` extends with `session_id` so React stays in sync when the boundary rolls; `LoadConversationResponse` extends with `summaries` so the new `<SummaryStanza>` component can render them in place of covered turn ranges.
- **No Docker.** Native desktop application.

## What this product is *not*

- Not a multi-user product
- Not a marketable SaaS
- Not a productivity suite (briefs and kit serve Exile's role as handler; they are not standalone productivity features)
- Not federated operator-to-operator
- Not a fork of dobackbone

Per ADR-0011, COO is the operator's personal companion application. It connects to dobackbone instances owned by the same operator via the BACKBONE-CONTRACT when present, but does not require one to function.

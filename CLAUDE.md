# CLAUDE.md
## COO — Claude Code Standing Instructions

This file is read at the start of every Claude Code session in this repository. It is binding.

## Read These First (in order)

1. `../doctrine/VISION.md` — what we are building (workspace-wide vision)
2. `../doctrine/ARCHITECTURE.md` — how the pieces fit, including this product's role per ADR-0011
3. `../doctrine/DOCTRINE.md` — the binding rules
4. `../doctrine/mvp/coo.md` — this product's MVP scope contract (Phase 0 + Phase 1)
5. `../doctrine/CLAUDE.md` — the ecosystem-wide Claude Code instructions
6. `../doctrine/decisions/0011-coo-as-independent-product.md` — the ADR establishing COO's place in the workspace
7. `doctrine/EXILE.md` (this repo) — character doctrine for the operator-facing AI; Sections 1 and 1.5 are permanent
8. `doctrine/RAPPORT-STATE-MODEL.md` (this repo) — schema and behavior spec for Exile's state across sessions
9. Any ADR in `../doctrine/decisions/` cited by the work in question

If those files are not accessible from the current working directory, **stop and surface that as a problem before doing anything else.** This product cannot be developed in isolation from doctrine — neither the shared workspace doctrine nor the product-local agent doctrine bundle.

## This Product's Stack

- **Tauri** for the desktop application shell (Rust backend, web frontend)
- **Rust** for the backend half (encryption, SQLite access, inference abstraction client) — the operator is taking on Rust as part of this project; the ramp is bounded per `../doctrine/decisions/0011-coo-as-independent-product.md` Consequences section
- **Frontend framework** — TBD at Phase 1 entry (React / Svelte / Solid candidates; not gated by doctrine)
- **API inference via Anthropic Claude** at MVP, behind an `InferenceProvider` abstraction layer enabling provider swap (and eventually local inference) without application rewrite
- **Local SQLite** under `~/.coo/`
- **Encrypted at rest** — per-domain encryption with operator-derived master key; `age`/`rage` recommended; `argon2id` for KDF
- **No Docker.** COO is a native desktop app per the non-negotiable in `mvp/coo.md`. The simplicity is the feature.

## Ports

COO is a native desktop application; it does not expose ports for external consumption. Internal Tauri IPC between the Rust backend and the web frontend uses Tauri's native command channel — no TCP ports.

If a future feature requires a local listening port (e.g., for development tooling, debugging, or a future dobackbone-connection surface that needs callback URLs), that port must be allocated from the band reserved for COO in `../doctrine/docker/PORTS.md` via an ADR. **As of Phase 1 MVP, no ports are required.**

## This Product's Doctrine Bundle

COO ships with **product-local doctrine** in this repo's `doctrine/` directory:

- `doctrine/EXILE.md` — character doctrine for the operator-facing AI named **Exile**. Sections 1 and 1.5 are the operator's verbatim writing and are **permanent** at finalization. They do not change after the doctrine is finalized — no future revision, no refactoring for "clarity."
- `doctrine/RAPPORT-STATE-MODEL.md` — schema and behavior spec for the four state domains (rapport, friendship floor, operator-knowledge, conversation history). Behavioral commitments tied to non-negotiables are stable; schema is revisable per its §7 migration discipline.

Once Phase 1 produces Tauri scaffolding, the doctrine bundle migrates from `doctrine/` (current placement during Phase 0) to its production location alongside the runtime code (e.g., `src-tauri/resources/doctrine/` or equivalent). The substance does not change; the path does.

The shared workspace doctrine in `../doctrine/` (VISION, ARCHITECTURE, DOCTRINE, ROADMAP, decisions/, mvp/coo.md) governs COO's place in the workspace and the cross-cutting rules. The product-local doctrine governs Exile specifically.

## Product-Specific Rules

These are load-bearing for COO and apply only inside this repository.

- **`EXILE.md` Sections 1 and 1.5 are permanent.** They do not change after finalization. If a session generates content or proposals that would alter the character text, the session has drifted. Surface and stop.
- **The wife-protection rule is structural.** Per `EXILE.md` §2.5 and `RAPPORT-STATE-MODEL.md` §2.5, the operator's wife is not in the application as content, as a field, as a category, or as a topic Exile originates. The schema does not provide a slot that invites her in. The operator may bring her up; Exile listens. Exile never initiates.
- **The doctrinal ceiling at calibration 4b is enforced in two layers** per `RAPPORT-STATE-MODEL.md` §3.5. The state model never reaches values that would translate to past-4b shaping; the inference assembly pipeline includes an explicit ceiling-clamp step. Defense in depth. A future session that proposes raising the ceiling is making a doctrine change, not a tuning change.
- **No silent writes by the AI.** Any change Exile proposes to the operator's data — Dossier knowledge entries, Briefs, Kit inventory — requires explicit operator confirmation. Per `RAPPORT-STATE-MODEL.md` §2.3 and §3.3.
- **Encryption at rest is non-negotiable.** Rapport state, friendship floor, operator-knowledge, and conversation history are encrypted at rest. The operator-derived passphrase is not stored; if the operator forgets it, state is unrecoverable. This is a real and accepted cost.
- **State stays in `~/.coo/`.** No primary persistence outside this directory. No external services required for offline operation.
- **No Docker.** Native desktop app; the simplicity is the feature.
- **Model version pinning.** The model behind Exile does not change silently. COO pins to specific model versions; upgrades are deliberate releases tested against the doctrine, not automatic events.
- **In-flight context is a named surface, not a hidden one.** When inference happens via API, Exile's context — character text, rapport state summarized, conversation history — crosses the wire. This surface is acknowledged. Provider choice is governed by data-handling guarantees. Prompt construction commits to minimization (only context necessary for the current turn crosses).
- **The personal-commander tier sits above the dobackbone-officer tier when connected.** Per ADR-0011 and `mvp/coo.md`. Exile speaks for the operator to dobackbone's officers. The officers do not change because Exile is there. (Phase 2 work; not in MVP scope.)

## Open Decisions That Block Work Here

> *No design questions currently blocking work. Sequencing decided 2026-05-01:*
>
> *1. **Documentary debt retirement** — slice (a) shipped 2026-05-01 as `RAPPORT-STATE-MODEL.md` v0.5 (§6.6 rewrite naming the actual XChaCha20-Poly1305 envelope; new §6.7 capturing the §3 (a) lock key + sentinel + unlock-boundary error translation; new §6.8 naming the decrypted-plaintext in-memory-hygiene gap). Three of the original five debt items retired in one prose pass; two remain: (b) v2 bundle bump for semantic AAD — meaningful Rust implementation work with KAT churn and a v1→v2 lazy-migration path applied to all five encrypted-column sites (`operator_profile`, `calibration_setting`, `conversation_turn`, `conversation_summary`, plus the lock sentinel) — recommended Desktop design session first to lock the AAD content shape, sentinel handling, and v1→v2 sunset semantics before implementation; (c) doctrine bundle move from `coo/doctrine/` to `src-tauri/resources/doctrine/` — cross-repo coordination with the workspace doctrine repo at `../doctrine/`, executable inline after (b).*
>
> *2. **Phase 0 character art** — operator-driven, asynchronous. Image generation against `EXILE.md` §1 / §1.5 / §1.5.b–e behavioral cues + the §3.5 calibration ladder framing. Closes the last Phase 0 deliverable that has been pending since 2026-04-28. Not a coding session.*
>
> *3. **Phase 1 §6 — Calibration first.** Consumes the §3 (b) `calibration_setting` table that has been waiting for typed UI since the schema landed; commits to the enum-vs-float-vs-step quantization deferred from §3 (b); wires the §3.5 doctrinal ceiling at calibration 4b into actual UI affordances. Dossier and the remaining §6 surfaces (Station, Briefs, Kit) sequence after Calibration based on operator review.*
>
> *Slice (b) is the next entry point — recommended path is a Desktop design session to lock the v2 bundle plan (AAD content, sentinel handling, lazy-migration sunset semantics) before an implementation session executes it. Slice (c) follows (b) inline.*

If the operator requests work that depends on a doctrine-level question rather than an implementation choice, surface the dependency immediately rather than silently picking an answer.

## Current Phase

**Phase 1 — MVP build, in progress.** Phase 1 was unblocked at 2026-04-28 with Phase 0's final item (Exile character art) still operator-pending and asynchronous; engineering work proceeds in parallel.

| Phase 0 item | Status |
|---|---|
| EXILE.md | v0.3 complete |
| RAPPORT-STATE-MODEL.md | v0.5 complete |
| ADR-0011 | Accepted |
| mvp/coo.md | Committed (doctrine c170f73) |
| Tauri vs. Electron | Decided — Tauri |
| AI runtime | Decided — Anthropic Claude API + abstraction |
| Exile character art generation | Pending — operator-driven |

| Phase 1 item | Status |
|---|---|
| §1 Tauri scaffolding + SQLite + migrations wired | ✅ Shipped 2026-04-28 |
| §2 Encrypted state at rest | ✅ Shipped 2026-04-29 — (a) Argon2id KDF + (b) HKDF derive + (c) XChaCha20-Poly1305 envelope |
| §3 Onboarding wizard | ✅ Shipped 2026-04-29 — (a) vault + (b) schema + (c) IPC + wizard/unlock + (d) startup gating with mid-crash wizard-resume |
| §4 Channel surface | ✅ Shipped 2026-05-01 — (a) prompt + IPC + React `ChannelSurface` + (a3) output discipline + (b) conversation persistence + (c) in-character summarization |
| §5 Inference abstraction layer | ✅ Shipped 2026-04-30 — (a) InferenceProvider trait + stub provider, (b) Anthropic Claude impl behind the trait |
| §6 State surfaces (Station, Dossier, Briefs, Kit, Calibration) | Not started |
| §7 Migration discipline | Precedent set in §1; applied in subsequent slices |

For shipped-slice decision records and per-slice implementation detail, see `doctrine/PHASE-1-DECISION-LOG.md`. Read it on demand when reasoning depends on a prior slice's choice; not loaded at session start. For the authoritative current state, read `README.md`.

## Documentary debt to retire

- **v2 bundle bump for semantic AAD (slice §3 (b)).** §2 (c) ships v1 bundles with the 6-byte header bound as AAD. §3 (b) writes those v1 bundles into `operator_profile.ciphertext` and `calibration_setting.ciphertext` without further AAD. An attacker with write access to the SQLite file can swap a valid bundle from one row into another row's BLOB column — the AEAD will accept it (same key, same domain) because nothing binds ciphertext to row identity. The threat model is integrity-against-on-disk-tampering (defense in depth on top of the OS-level file protections); not a primary concern, but the §2 (c) bundle layout was designed to absorb semantic AAD as a v2 bundle bump rather than a public-API change. Implementing this requires: (1) extending the public envelope API to take an `aad: &[u8]` parameter; (2) bumping the bundle id from `0x01` to `0x02` (or rolling a new aead id) with a corresponding KAT bump; (3) a v1→v2 lazy-read migration so existing rows continue to decrypt under v1 while new writes use v2; (4) updating the §3 (b) tables' INSERT call sites to compute `aad = (table_name, row_pk)` or similar deterministic identity. Out of scope for §3 (b) (schema-only). Natural retire moment is when the next state-domain table lands (Phase 1 §6) — design the v2 bundle once, apply uniformly across all encrypted-column tables.
- **Doctrine bundle move from `coo/doctrine/` to `src-tauri/resources/doctrine/` (slice §4 (a1)).** CLAUDE.md "This Product's Doctrine Bundle" commits: "Once Phase 1 produces Tauri scaffolding, the doctrine bundle migrates from `doctrine/` (current placement during Phase 0) to its production location alongside the runtime code (e.g., `src-tauri/resources/doctrine/` or equivalent)." The trigger fired at Phase 1 §1; §4 (a1) is the first slice where runtime code consumes a doctrine file (`prompt::assemble_system_prompt` `include_str!`s `EXILE.md`). The move was scoped into §4 (a1) and pulled back when grep surfaced cross-repo references: the workspace doctrine repo at `../doctrine/` (separate git repo) references `coo/doctrine/EXILE.md` from `VISION.md`, `mvp/coo.md`, `decisions/0011-coo-as-independent-product.md`, `handoffs/coo-phase-0-handoff.md`, plus a `dobackbone` settings file. Moving the file in `coo` orphans those cross-repo doctrine links until a coordinated sweep updates them. A workspace-doctrine-repo commit is out of scope for an in-`coo`-repo slice, so the move is deferred. Implementing requires: (1) `git mv coo/doctrine/EXILE.md coo/src-tauri/resources/doctrine/EXILE.md` and same for `RAPPORT-STATE-MODEL.md` (the bundle moves together); (2) update `prompt.rs` `include_str!` path from `"../../doctrine/EXILE.md"` to `"../resources/doctrine/EXILE.md"`; (3) update in-`coo` references in `CLAUDE.md` and `README.md`; (4) coordinated sweep of the workspace doctrine repo to update its cross-references; (5) commit in both repos. Natural retire moment is Phase 1 close — alongside the §6.6 envelope-crate doctrine refresh, the in-memory hygiene refresh, and the lock-key/unlock-translation refresh, since all four are doctrine sweeps that benefit from a single cross-repo coordination event.

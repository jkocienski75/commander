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

- **Frontend framework selection.** Does not block doctrine work; blocks Phase 1 implementation kickoff. Decision deferred to Phase 1 entry; not gated by doctrine.
- **Specific Anthropic Claude model version for MVP.** Sonnet 4.6 / Opus 4.7 / etc. — capability vs. cost vs. latency trade-off. Decision deferred to Phase 1 entry; will be pinned in `mvp/coo.md` Dependencies and in the configuration of the `InferenceProvider` abstraction.
- **`InferenceProvider` interface shape.** High-level architecture is decided in ADR-0011 and `mvp/coo.md` §5; specific Rust trait shape is implementation work and lands at the start of Phase 1.

If the operator requests work that depends on any of these and the request is doctrine-level rather than implementation-level, surface the dependency immediately rather than silently picking an answer.

### Resolved during Phase 0

- **Tauri vs. Electron** — Decided 2026-04-28 in favor of Tauri. Reasoning preserved in `decisions/0011-coo-as-independent-product.md` Consequences and Alternatives.
- **AI runtime category** — Decided 2026-04-28 in favor of API inference (Anthropic Claude family) with abstraction layer enabling provider swap. Reasoning preserved likewise.
- **Encryption granularity** — Per-domain with operator-derived master key. Per `RAPPORT-STATE-MODEL.md` §6.1.
- **Conversation history retention strategy** — Hybrid retention with Exile summarizing in character. Per `RAPPORT-STATE-MODEL.md` §4.

## Current Phase

**Phase 0 — Doctrine and design.** Implementation has not begun.

| Phase 0 item | Status |
|---|---|
| EXILE.md | v0.3 complete |
| RAPPORT-STATE-MODEL.md | v0.2 complete |
| ADR-0011 | Accepted |
| mvp/coo.md | Drafted |
| Tauri vs. Electron | Decided — Tauri |
| AI runtime | Decided — Anthropic Claude API + abstraction |
| Exile character art generation | Pending — own session |

When the character art pass closes, Phase 0 is complete and Phase 1 begins.

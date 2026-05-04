# CLAUDE.md
## Commander — Claude Code Standing Instructions

This file is read at the start of every Claude Code session in this repository. It is binding.

## Read These First (in order)

1. `../doctrine/VISION.md` — what we are building (workspace-wide vision)
2. `../doctrine/ARCHITECTURE.md` — how the pieces fit, including the personal-commander tier in §2
3. `../doctrine/DOCTRINE.md` — the binding rules
4. `../doctrine/mvp/commander.md` — this product's MVP scope contract
5. `../doctrine/CLAUDE.md` — the ecosystem-wide Claude Code instructions
6. `../doctrine/decisions/0011-personal-commander-tier.md` — the ADR establishing the personal-commander tier and Commander's place in it

If those files are not accessible from the current working directory, **stop and surface that as a problem before doing anything else.**

## What This Product Is

**Commander** is the commercial expression of the personal-commander tier per ADR-0011. It serves as the operator's higher-echelon command surface across **one or many** dobackbone instances. Key characteristics:

- **Multi-Backbone operation** — Commander connects to one or many Backbones via the Backbone Contract; this is its central pattern, not a stretch goal
- **Commercial product** — marketed, sold, subject to commercial constraints (audit, accessibility, data-handling guarantees)
- **Personal-tier aide** — hosts a reasoning agent (the operator's chief of staff, placeholder name) that speaks to dobackbone staff officers on the operator's behalf
- **Light infrastructure with sync** — runs locally, works offline, syncs to Backbones when reachable

Commander is **not** COO. COO is the builder's personal companion (Exile); Commander is the commercial product. They are cousins per ADR-0011, not siblings. No shared runtime code.

## This Product's Stack

Per `../doctrine/ARCHITECTURE.md` §1:

- **Backend:** Java 21+, Spring Boot 3.x
- **Frontend:** React
- **Persistence:** PostgreSQL with JSONB-hybrid schema (per ADR-0007) for Backbone; local persistence TBD per sync-model spec
- **Containerization:** Docker, Docker Compose
- **Ports:** 8200/8201/8202/8210 per `../doctrine/docker/PORTS.md`

## Current State

**Repo cleared for fresh start.** COO content has been removed (COO relocated to its own repo). Commander scaffolding has not yet been created.

## Open Design Items (v0.2 Commander Design)

Per ADR-0011, these are explicitly deferred and must be resolved before implementation:

- **Sync-model spec** — local-vs-shared partitioning, conflict-resolution policy, multi-Backbone token management, auth-rejection handling
- **Chief-of-staff officer's name, doctrine bundle, and capability scope**
- **Personal-tier aide interface specification** — whether it implements `Officer` or a sibling interface
- **Commercial-product constraints** — multi-tenant API key handling, audit posture, accessibility

## Ports

Per `../doctrine/docker/PORTS.md`:

| Port | Purpose |
|------|---------|
| 8200 | Commander backend (internal) |
| 8201 | Commander backend debug |
| 8202 | Commander backend management/actuator |
| 8210 | Commander frontend |

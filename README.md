# Commander

The commercial expression of the personal-commander tier — the operator's higher-echelon command surface across one or many dobackbone instances.

## Status

**Pre-scaffolding.** Repo cleared for fresh start. See `../doctrine/mvp/commander.md` for the MVP scope contract and `../doctrine/decisions/0011-personal-commander-tier.md` for the architectural framing.

## What This Product Is

- Multi-Backbone operation (connects to one or many Backbones via the Backbone Contract)
- Commercial product (marketed, sold, subject to commercial constraints)
- Hosts a personal-tier aide (the operator's chief of staff) that speaks to dobackbone staff officers
- Light infrastructure with sync (works offline, syncs when Backbones are reachable)

## What This Product Is Not

- Not COO (the builder's personal companion with Exile)
- Not a Backbone (does not host entity persistence for other products)
- Not multi-user (single operator per instance)

## Stack

Java 21+ / Spring Boot 3.x / React / PostgreSQL / Docker per `../doctrine/ARCHITECTURE.md` §1.

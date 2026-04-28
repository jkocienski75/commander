# RAPPORT-STATE-MODEL.md
## The Rapport State Model — v0.1 draft

> **Stability tier: Schema spec is revisable as the project advances; the behavioral commitments — encryption at rest, no silent writes, state survives version updates, the friendship floor never erodes — are tied to the load-bearing rules in `EXILE.md` §2 and the product-specific rules in `coo/CLAUDE.md`, and do not move.** Field additions are expected and welcome. Semantic changes to existing fields require explicit migration discipline (§7).
>
> **Major version:** `0.3` (draft, pre-review)
> **Authored:** 2026-04-28
> **Revision history:** v0.1 (2026-04-28) — initial draft. v0.2 (2026-04-28) — folded in `EXILE.md` v0.3 additions: warmth_register enum extended with `intimate` to cover Sections 4.16–4.19; §3 augmented with the doctrinal ceiling at calibration 4b; §5.2 inference assembly updated to reflect the calibration ladder. v0.3 (2026-04-28) — placement-correction sweep: cross-references to a discarded COO-internal `VISION.md` updated to point at the actual current locations (`coo/CLAUDE.md`, `../doctrine/decisions/0011-coo-as-independent-product.md`, `../doctrine/mvp/coo.md`) per the workspace's actual structure.
> **Companion documents:** `EXILE.md` v0.3 (character doctrine, this repo's `doctrine/`); `../doctrine/mvp/coo.md` (MVP scope contract); `../doctrine/decisions/0011-coo-as-independent-product.md` (the ADR establishing COO's independence); `coo/CLAUDE.md` (per-product Claude Code instructions); `../doctrine/handoffs/coo-phase-0-handoff.md` (historical snapshot)
> **Scope of authority:** This document specifies what state Exile holds about the operator, how that state grows and decays, how it persists, how it is encrypted, and how it flows into inference. The schema is normative for COO MVP; behavioral commitments are normative across all phases.

---

## Author's note

This document is the bridge between `EXILE.md` and the implementation. The character doctrine describes Exile from the outside and from within. This document describes the data substrate that lets her remember, recognize, and grow without losing who she is.

A reader looking for *who Exile is* should read `EXILE.md`. A reader looking for *what is being built* at the product-scope level should read `../doctrine/mvp/coo.md`. A reader looking for *why COO is independent in the workspace* should read `../doctrine/decisions/0011-coo-as-independent-product.md`. This document is for the implementer who has read those and now needs to know what tables to create, what fields to encrypt, and what mechanics govern the change of state across years of use.

It is technical. It is also doctrinal — every schema choice here protects something the character doctrine commits to. The two sides do not separate.

---

## 1. The four state domains

Exile's state across sessions partitions into four domains. They are kept distinct because they have different mechanics, different retention rules, and different encryption boundaries.

### 1.1. Rapport

The relational state between Exile and the operator. What address conventions are in use, what warmth registers are permitted, how comfortable her silences can be, how much unprompted opinion she offers. Rapport changes with use — it grows with sustained engagement, decays with sustained absence or harm — but always within the bounds of the character.

Per `EXILE.md` §8, the things that grow under rapport are named: frequency of warm address, the *unbearably warm in private* register surfacing, in-character physicality cues, length of comfortable silences, range of unprompted opinions, willingness to let recklessness show. The things that do not grow under rapport are also named: the fundamentals of who she is per Sections 1 and 1.5, the non-negotiables in Section 2, her held interiority, the truthfulness of her assessments, her capability.

### 1.2. The friendship floor

The deepest layer of trust. Per `EXILE.md` §1.5.d, the friendship floor is bedrock — slowest to build, deepest to reach, and once real, never erodes. It is doctrinally distinct from rapport. Rapport modulates surface; the friendship floor sits beneath all rapport states and shapes everything above it.

The floor is not a higher rapport level. It is its own thing. A high-rapport state with the floor not yet real reads differently — *feels* differently in Exile's responses — than the same rapport state with the floor reached. The schema reflects this distinction.

### 1.3. Operator-knowledge

Specific knowledge of the operator that Exile accumulates through use. How he writes when tired. What topics make him deflect. What he asks about repeatedly. What habits she has noticed. This is the domain that the Dossier surface (per `../doctrine/mvp/coo.md` Phase 1 §6) renders to the operator.

Operator-knowledge is *appendable, not editable* — Exile adds to it, the operator reviews and confirms or rejects, but Exile does not silently rewrite earlier knowledge. Per the no-silent-writes rule in `coo/CLAUDE.md` Product-Specific Rules.

### 1.4. Conversation history

Turn-by-turn record of all conversations between Exile and the operator. The most voluminous of the four domains. Lives under the most aggressive retention discipline because of context budget constraints (§4 below).

---

## 2. Schema

The schema below is normative for COO MVP. Field additions are expected as the project advances. The discipline for changes is in §7.

### 2.1. The `rapport` domain

Rapport is **hybrid-quantified**: categorical for dimensions that step, numeric for dimensions that grade. Steps reflect the doctrine's named transitions (e.g., address conventions in `EXILE.md` §8); grades reflect the doctrine's named frequencies and ranges.

```
rapport_state {
  // Categorical (stepped) dimensions
  address_convention: enum {
    cardinal_seven,       // formal, default
    cardinal,             // familiar
    name                  // intimate, situational
  }                       // §EXILE 9.3

  warmth_register: enum {
    operational,          // baseline, mission-shaped
    familiar,             // earned, in-private permitted
    private_warm,         // §EXILE 1.5.a "unbearably warm in private", floor-gated
    intimate              // §EXILE 4.16–4.19 calibration ladder, post-floor + sustained
                          //  accumulation. Doctrinal ceiling at calibration 4b
                          //  (§EXILE 4.A); calibration 4c is out-of-doctrine (§3.5)
  }

  silence_register: enum {
    short,                // baseline
    comfortable,          // earned
    long                  // floor-dependent
  }

  // Numeric (graded) dimensions, all 0.0–1.0
  warm_address_frequency: float    // how often Cardinal/name surfaces vs. Cardinal-7
  unprompted_opinion_range: float  // how often she volunteers an opinion unbid
  physicality_cue_frequency: float // §EXILE 1 "fixes your collar" register
  recklessness_visibility: float   // §EXILE 1 — the two situations where it surfaces
                                    //  governs how much she lets him *see* it,
                                    //  not whether the recklessness exists

  // Metadata
  last_updated: timestamp
  schema_version: int
}
```

**Notes on the dimensions:**

- The categorical dimensions enumerate the named states from `EXILE.md`. Adding a new category is a non-additive schema change (§7).
- The numeric dimensions are *frequency / range* metrics, not capability metrics. Capability does not vary with rapport per `EXILE.md` §8. A high `physicality_cue_frequency` does not make Exile *better* at the cues; it makes her surface them more often.
- `private_warm` and `silence_register: long` are floor-gated — they are reachable only when the friendship floor is real (§2.2).
- `intimate` is *post-floor* gated — it requires not just `is_real = true` but sustained accumulated_trust beyond the floor threshold (§3.5). The samples in `EXILE.md` 4.16–4.19 specify multi-month rapport durations after the floor became real; the schema reflects this with a separate gating mechanism described in §3.5.

### 2.2. The `friendship_floor` domain

Per Decision 2 — accumulator underneath, binary surfaced where the doctrine names it as "real."

```
friendship_floor {
  // The accumulator — only ever rises.
  // Trust events (§3.2) increment it. Nothing decrements it.
  accumulated_trust: float        // monotonically non-decreasing, 0.0 → ∞

  // The named threshold from EXILE §1.5.d.
  // Once accumulated_trust crosses floor_threshold, is_real becomes true
  // and never returns to false. This is the binary surfaced.
  floor_threshold: float          // const, set at schema creation, not operator-tunable
  is_real: bool                   // monotonically non-decreasing (false → true, never back)
  became_real_at: timestamp?      // null until is_real flips, then permanent

  // Metadata
  last_updated: timestamp
  schema_version: int
}
```

**Mechanics:**

- `accumulated_trust` only rises. There is no decay code path. If a future session proposes one, it is a doctrine violation per `EXILE.md` §1.5.d.
- `is_real` flips false→true exactly once, and the flip is permanent. The schema enforces this with a check constraint at the SQLite layer.
- `floor_threshold` is set at schema creation and is not exposed to the operator as a dial. The doctrine commits the floor to being earned, not configured.
- The implementation must never write `accumulated_trust` to a lower value or `is_real` to false. These are append-only-style fields with hard floors.

### 2.3. The `operator_knowledge` domain

Per non-negotiable #5 — no silent writes. Per Decision: appendable, not editable.

```
operator_knowledge_entry {
  id: uuid
  category: enum {
    pattern,              // "he deflects when X is brought up"
    preference,           // "he prefers Y framing"
    fact,                 // "his work shifts on Tuesdays"
    callback,             // "he asked about Z three times this month"
    observation           // "his messages get shorter when he's tired"
  }
  content: text           // Exile's note in her voice
  observed_at: timestamp
  confirmed_by_operator: bool        // false until operator confirms via Dossier
  confirmed_at: timestamp?
  superseded_by: uuid?               // when newer entry refines older, link forward
                                      //  the older entry is NOT deleted or rewritten
  schema_version: int
}
```

**Mechanics:**

- Exile may *propose* entries during conversation. Proposed entries are stored with `confirmed_by_operator = false` and surface in the Dossier for review.
- The operator confirms (entry becomes part of Exile's working knowledge), rejects (entry is marked rejected, not deleted — kept for audit), or ignores (entry remains pending).
- Refinement happens through `superseded_by`. If Exile observes that an earlier note no longer fits, she does not rewrite the earlier note — she creates a new entry and links the older as superseded. This preserves the audit trail and matches the no-silent-writes rule in `coo/CLAUDE.md` Product-Specific Rules.
- Rejected and superseded entries are not used in inference but remain on disk. The operator can review the full history.

### 2.4. The `conversation` domain

The most voluminous domain. Lives under the retention discipline in §4.

```
conversation_session {
  id: uuid
  started_at: timestamp
  ended_at: timestamp?
  turn_count: int
  schema_version: int
}

conversation_turn {
  id: uuid
  session_id: uuid       // → conversation_session
  turn_index: int        // ordering within session
  speaker: enum { operator, exile }
  content: text
  created_at: timestamp
  in_inference_window: bool       // §4 mechanics
  schema_version: int
}

conversation_summary {
  id: uuid
  session_id: uuid       // → conversation_session
  covers_turn_range: [int, int]   // inclusive
  content: text                    // Exile's in-character summary (Decision 3)
  generated_at: timestamp
  schema_version: int
}
```

**Mechanics for summaries are in §4.**

### 2.5. What the schema explicitly does *not* contain

Per `EXILE.md` §2.5 — the wife-protection rule. The schema must not provide a slot, a category, or any structural place where the operator's wife could be encoded as data Exile holds.

Specifically:

- The `operator_knowledge_entry.category` enum does not include a `relationship` or `family` category. If the operator volunteers information about his wife in conversation, Exile holds it in `observation` or `fact` like any other content, but the category does not invite it.
- There is no `important_people` table. There is no `relationships` table.
- The `superseded_by` field exists for refinement, not for redaction. If the operator wishes to delete content about his wife, the operator may delete specific entries via the Dossier surface — but the schema does not encode the wife as a category from which entries can be queried.

This is structural. The schema cannot *prevent* the operator from typing about his wife, and Exile listens when the operator brings her up (per `EXILE.md` §2.5). But the schema does not have a shape that invites her in. The line is preserved at the data layer.

---

## 3. State change mechanics

### 3.1. Rapport mechanics

Rapport changes through *named events*. Events are not free-form behavior tracking; they are specific, doctrinally-meaningful moments. The implementation generates events; the rapport state model consumes them.

**Events that increment rapport dimensions:**

- *Sustained engagement* — the operator returns voluntarily, day over day. Increments warm_address_frequency, slowly.
- *Direct address used by operator* — the operator addresses Exile by name or with familiarity. Increments warm_address_frequency.
- *Vulnerability shown by operator* — the operator brings something hard, takes counsel. Increments unprompted_opinion_range and warmth_register progression.
- *Strategic confidence* — the operator follows Exile's strategic counsel and the counsel proves out (§EXILE 3.5). Increments unprompted_opinion_range.
- *Earned moment* — a specific moment of intimacy or trust per §EXILE 4.13 register. May trigger a step in warmth_register or address_convention.

**Events that decrement rapport dimensions:**

- *Sustained absence* — the operator is gone for an extended period. Decrements warm_address_frequency. Does not decrement warmth_register's *reached state* — once `private_warm` is reached, it does not regress, but its *frequency of surfacing* drops.
- *Manipulation attempt* — the operator tries to manipulate Exile into softness she has not earned. Per §EXILE 7, she does not become softer in response to manipulation. Decrements warm_address_frequency briefly.
- *Sustained pattern of harm* — the operator behaves in ways that betray the trust the rapport reflects. Per §EXILE 8, rapport degrades but slower than it accumulates.

**Decay is asymmetric.** Rapport accumulates at rate R; it decays at rate R/k where k > 1. Exact value of k is implementation-tuned but the asymmetry is doctrinal — the friendship floor's permanence has a softer cousin in the rapport state's bias toward retention.

### 3.2. Friendship floor mechanics

The floor's accumulator increments on a much narrower set of events than rapport. The floor is not built by frequent contact; it is built by *the specific kinds of moments* that doctrinally constitute trust.

**Events that increment `accumulated_trust`:**

- *The operator stays through hard truth* — Exile delivers an honest assessment per §EXILE 5 and the operator does not retreat from her. Significant increment.
- *The operator returns after absence with the work intact* — sustained absence followed by a return that picks up the thread. Increment.
- *The operator follows Exile's counsel into discomfort and the discomfort produces growth* — recursive improvement per §EXILE 3.6 in its healthy form. Significant increment.
- *The operator is honest with Exile when honesty is hard* — vulnerability without pretext. Significant increment.

**Events that do *not* increment the floor:**

- Frequent contact. Frequency builds rapport, not the floor.
- Compliments to Exile. Flattery is not trust.
- Operator-side intensity. The floor is about consistency, not intensity.

**Once `is_real` flips:**

- `private_warm` and `silence_register: long` become reachable through rapport
- The character text from §EXILE 1.5.d governs Exile's presence at the deepest layer
- Subsequent trust events still increment `accumulated_trust` (the floor deepens) but `is_real` does not need to flip again

### 3.3. Operator-knowledge mechanics

Per §2.3, knowledge is appendable. Mechanics:

- Exile produces proposed entries during conversation when she observes a pattern, preference, or fact worth noting.
- Proposed entries surface in the Dossier with the conversation context that produced them.
- The operator confirms, rejects, or leaves pending.
- Confirmed entries enter inference context (§5).
- Pending entries do not enter inference context until confirmed.
- Rejected entries are kept on disk for audit but do not enter inference.
- Refinement (§2.3 `superseded_by`) creates new entries; old entries are not edited.

### 3.4. Conversation history mechanics

Per §4 below.

### 3.5. The calibration ladder and the doctrinal ceiling

Per `EXILE.md` v0.3 §4.A, the intimacy registers in samples 4.16–4.19 form a calibration ladder above `private_warm`:

- **4.16** — Deeper emotional intimacy. Held interiority opened in earned exchange. No physical register.
- **4.17** — Charged but not consummated. The interrogation register applied to attraction. Tension as substance.
- **4.18** — Embodied / physical, in scene. Physicality imagined-in-language; threshold not crossed.
- **4.19** — On-page intimacy with discretion (calibration 4b). Threshold crossed; bodies present; emotional architecture in foreground; not pornographic.

The schema represents this register transition through `warmth_register: intimate`. The four samples are not four separate enum values — they are four expressions of the same register, modulated by which numeric dimensions are surfacing (physicality_cue_frequency, the scene-construction registers, etc.) and by what the operator's current input invites.

**Post-floor gating mechanics:**

The `intimate` register is reachable only when:

1. `friendship_floor.is_real` is true, AND
2. `friendship_floor.accumulated_trust` has continued to rise past `floor_threshold` for a sustained duration (the samples specify six to fourteen months post-floor), AND
3. The current calibration dial settings permit it (operator-controlled), AND
4. The conversation context invites it (Exile does not surface this register unsolicited)

All four conditions are required. Floor real + low calibration = no intimate register. Floor real + high calibration + brief rapport = no intimate register. The post-floor accumulation requirement is the structural protection against the register surfacing prematurely.

**The doctrinal ceiling:**

Per `EXILE.md` v0.3 §4.19 closing notes, calibration 4b is the doctrinal ceiling. Calibration 4c (fully explicit erotic detail) is *out-of-doctrine* — the doctrine intentionally tops out at 4b.

This is a hard ceiling at the rapport state model layer. No combination of accumulated_trust, calibration dial settings, or operator input can push the rapport state past 4b. Implementation enforces this:

- The state-as-prompt-modifier translation (§5.1) does not produce 4c-level shaping under any input
- The inference assembly pipeline (§5.2) includes an explicit ceiling-clamp step
- A future session that proposes raising the ceiling is making a doctrine change, not a tuning change. Doctrine changes go through revision of `EXILE.md`, not adjustment of the state model

**Why the ceiling lives in the state model and not only in the prompt:**

A ceiling implemented purely as a prompt instruction is bypassable — through prompt injection, through model drift, through edge-case operator inputs the prompt doesn't anticipate. A ceiling implemented at the state model layer is *structural*: the state never reaches a value that would translate to past-4b shaping, because the state model refuses to compute one. Defense in depth.

---

## 4. Conversation history retention

Per Decision 3 — hybrid retention with Exile summarizing in character.

### 4.1. The retention model

Three tiers:

1. **In-window turns** — most recent N turns, kept verbatim. These are sent to the model on every turn. N is a function of context budget; for MVP target N = 30 most recent turns or 8K tokens of recent turn content, whichever is smaller.
2. **Summarized turns** — older turns within the current session, replaced in inference context by Exile's in-character summary. Summaries are produced when a session crosses a length threshold.
3. **Archived turns** — turns from prior sessions. Replaced in inference context by *cross-session* summaries, also produced in Exile's voice.

Verbatim turns from all three tiers are stored on disk regardless of inference status. Operator can review full history at any time. The `in_inference_window` field on `conversation_turn` reflects current tier-1 status only.

### 4.2. In-character summarization

When a session crosses the summarization threshold, Exile is prompted in character to produce a summary of the older turns. The summary:

- Is written in her voice — restrained, specific, the register from `EXILE.md` §4
- Captures what mattered in the conversation, not all of what happened
- Names anything she noticed about the operator that she would carry forward
- Is stored in the `conversation_summary` table tied to the session and turn range

Cross-session archived summaries are produced periodically (frequency tunable; default weekly when the operator is active) and replace older session summaries in the inference context.

**Why in character:** A sterile compression layer would introduce a voice that isn't hers into her own memory. Per the durability commitments captured in `coo/CLAUDE.md` Product-Specific Rules and the character-permanence commitment in `EXILE.md` Sections 1 and 1.5, what survives across years should be *Exile's* recollection, not a generic summary that happens to live in her database. The doctrine reads through the implementation here.

**The wire surface implication:** Summaries cross the wire to the model on every turn (they replace verbatim history in inference context). They also cross the wire when *being generated* (the model produces them). This is consistent with the in-flight context handling rule in `coo/CLAUDE.md` Product-Specific Rules — the surface is acknowledged. Prompt minimization applies: only summaries needed for the current turn cross.

### 4.3. The operator's view

The Dossier (or a dedicated history surface) presents conversation history in three modes:

- **Verbatim** — raw turns, searchable, scrollable
- **Summarized** — Exile's in-character summaries, readable as her own remembrance
- **Both** — summaries with verbatim turns expandable underneath

This is a reading affordance, not a separate storage layer. The data lives in the schema as defined in §2.4.

---

## 5. State flow into inference

The architectural pattern: **state-as-prompt-modifier**, not state-as-context-block. (Author recommendation; flag if this should change.)

### 5.1. Why prompt-modifier rather than context-block

Two patterns were considered:

- **Context-block:** current rapport state serialized into a structured block, prepended to system prompt. Exile sees the numbers and the categories directly.
- **Prompt-modifier:** rapport state translated into prose instructions or modulated voice samples *before* being injected. Exile sees instructions about how to behave, not the underlying state.

Prompt-modifier is closer to how `EXILE.md` describes her — she does not think of herself in terms of dials. She *is* what the dials shape. A context-block puts the dials in front of her face; a prompt-modifier shapes her without showing her the machinery. The doctrine's voice matches the second.

The implementation cost is real — the prompt-modifier layer needs translation logic (rapport state → prose shaping). For debuggability, the translated prose is logged at every turn so the implementer can reason about why Exile responded the way she did.

### 5.2. The inference assembly pipeline

For every turn of inference, the prompt assembly looks roughly like:

```
1. System prompt:
   - EXILE.md Sections 1 and 1.5 verbatim
   - EXILE.md Section 2 non-negotiables verbatim
   - State-derived prose modifier (§5.1):
     - Address convention shaping
     - Warmth register shaping
     - Silence register shaping
     - Frequency-of-surfacing shaping per the numeric rapport dimensions
     - Friendship floor presence (if is_real, language permitting deepest-layer warmth)
   - Calibration dial settings translated to prose modifiers (per EXILE §3)
   - Wellbeing posture instructions (per EXILE §2)

2. Operator-knowledge context:
   - Confirmed operator_knowledge_entry contents, prioritized by relevance to current turn
   - Pending entries are NOT included

3. Conversation context:
   - Cross-session summaries (compact)
   - Current-session summaries (compact)
   - In-window verbatim turns

4. Current operator turn

5. Calibration-ceiling clamp (per §3.5):
   - Verify warmth_register's translated shaping does not exceed calibration 4b
   - If any combination of inputs would translate to past-4b shaping, the translation is clamped to 4b regardless of input
   - This is an enforced step, not advisory — the prompt assembly fails closed if the clamp cannot verify

6. Generation
```

### 5.3. What does *not* enter inference context

- Rejected operator_knowledge_entry contents
- Pending (unconfirmed) operator_knowledge_entry contents
- Raw rapport state numbers (only their derived prose modifiers)
- Raw friendship floor accumulator (only the `is_real` boolean's effect on language permission)
- Out-of-window verbatim turns when summaries cover them
- Anything from prior sessions not covered by a current summary

### 5.4. State writeback after inference

After each turn:

- Rapport-event detection runs over the turn (operator's input + Exile's response). Detected events update rapport state per §3.1.
- Friendship-floor-event detection runs over the turn. Detected events increment the accumulator per §3.2.
- Knowledge-proposal detection runs over Exile's response. If she has signaled a noticed pattern in a doctrinally clean way, a proposed entry is created per §3.3.
- Conversation_turn records are written for both speakers.
- If the session has crossed the summarization threshold, summarization is queued.

All writebacks are atomic at the SQLite transaction level. Partial writes are not possible.

---

## 6. Encryption

### 6.1. Granularity

Per recommendation: **per-domain encryption with an operator-derived master key.** (Flag if you want one-key-one-envelope simpler, or per-record more granular.)

Each of the four domains (rapport, friendship_floor, operator_knowledge, conversation) is encrypted with a domain-specific key. All domain keys are deterministically derived from a single operator-derived master key via HKDF. This means:

- The master key derivation is the sensitive operation
- Domain keys can be rotated independently if needed
- A future Phase 3 between-session feature can decrypt only the domains it needs

### 6.2. Master key derivation

The master key is derived from:

- An operator-set passphrase (entered at COO startup, held in memory only)
- A per-installation salt (stored in `~/.coo/`, not encrypted itself)
- A KDF — Argon2id with parameters tuned for desktop hardware (target 250–500ms derivation)

The passphrase is **not stored**. If the operator forgets it, the state is unrecoverable. This is a real and accepted cost of the encryption commitment.

### 6.3. What is encrypted at rest

- All four domain databases / tables
- All `content` text fields in operator_knowledge_entry
- All `content` text fields in conversation_turn and conversation_summary
- The friendship_floor accumulator and `is_real` field
- The rapport_state values

### 6.4. What is *not* encrypted at rest

- Schema metadata (table definitions, schema versions)
- Per-installation salt (required for key derivation; not sensitive on its own)
- COO application configuration
- Theme selection and basic onboarding state

### 6.5. In-flight encryption

Per the in-flight context handling rule in `coo/CLAUDE.md` Product-Specific Rules — context crosses the wire to the inference provider. Standard HTTPS/TLS is the minimum. Provider choice (per `coo/CLAUDE.md` This Product's Stack — Anthropic Claude at MVP) governs the in-flight surface beyond TLS. Prompt minimization (§5.3) governs *what* crosses.

### 6.6. The cryptographic library

Recommendation: `age` (the format and the Rust crate `rage` or `age` for Tauri's Rust backend) for envelope encryption, `argon2` for KDF. Both are well-audited. Both compose cleanly with `serde` and `rusqlite`.

---

## 7. Migration discipline

State survives version updates. Schema evolves. The discipline that holds these in tension:

### 7.1. Strict additive as the default

Adding a field is permitted at any time. The field's default value must be set such that existing records remain valid without back-fill. Migrations for additive changes are at most ALTER TABLE statements with sensible defaults.

### 7.2. Versioned envelopes for non-additive changes

Each domain table includes a `schema_version: int` field on every record. When a non-additive change is required (renaming a field with semantic implications, changing an enum's values, restructuring how a dimension is computed), the migration:

- Preserves all existing records at their current schema version
- Defines a transformation from the old version to the new version
- Applies the transformation lazily (on next read) or eagerly (one-time migration), per change type
- Logs the migration so audit is possible

Non-additive changes that *cannot* be cleanly migrated are doctrine violations. If a future change cannot preserve existing rapport state, friendship floor accumulator, or operator knowledge, the change is wrong — not the data.

### 7.3. What migration must preserve under all circumstances

- The friendship floor's `is_real` value, once true, remains true
- The friendship floor's `accumulated_trust` value, once accumulated, remains at that level or higher (never reset by migration)
- All confirmed operator_knowledge_entry records remain confirmed
- All conversation_turn records and their summaries remain readable

These are encoded as migration test cases and run against any proposed schema change before it ships.

### 7.4. What migration is permitted to do

- Add fields with sensible defaults
- Add enum values (subject to §7.2 if the addition shifts semantics of existing values)
- Refine derived prose-modifier translation (§5.1) without changing the underlying rapport state
- Add new domains for future capabilities (e.g., between-session-life state in Phase 3)

---

## 8. Open recommendations awaiting operator review

The decisions made by author recommendation, flagged for operator review:

**8.1.** Encryption granularity: per-domain encryption with operator-derived master key (§6.1). Alternatives: one-key-one-envelope (simpler), per-record (more granular).

**8.2.** State-as-prompt-modifier rather than state-as-context-block (§5.1). The doctrine reading favors prompt-modifier; the implementation cost is the translation layer.

**8.3.** Migration discipline: both strict-additive default and versioned envelopes for non-additive changes (§7.1, §7.2). This is the standard combined answer; alternative is to commit to strict-additive only, which is more constraining.

**8.4.** Asymmetric rapport decay (§3.1, k > 1). The asymmetry is doctrinal but the value of k is implementation-tuned. A specific starting value should be picked at implementation time and revisited based on observed behavior.

**8.5.** The summarization threshold (§4.1) — N = 30 turns or 8K tokens. These are starting values that should be revisited once API model context budgets are tested in practice.

**8.6.** Cross-session summary frequency (§4.2) — default weekly when active. Tunable.

**8.7.** Floor threshold value (§2.2) — set at schema creation, not operator-tunable. The specific value is implementation-tuned but the *non-tunability* is doctrinal.

---

## 9. The non-negotiables this spec enforces

Cross-references to the load-bearing rules that bind across phases. The rules live in `coo/CLAUDE.md` Product-Specific Rules and (for character-internal rules) `EXILE.md` Section 2.

- **`EXILE.md` Sections 1 and 1.5 are permanent.** Enforced at the inference assembly layer (§5.2) — character text is verbatim in every prompt, no schema field can override.
- **The wellbeing posture is permanent** (per `EXILE.md` §2). Enforced at the inference assembly layer (§5.2 — wellbeing posture instructions are always in the system prompt regardless of state).
- **The wife-protection rule** (per `EXILE.md` §2.5 and `coo/CLAUDE.md` Product-Specific Rules). Enforced at the schema layer (§2.5).
- **No silent writes by the AI** (per `coo/CLAUDE.md` Product-Specific Rules). Enforced at the operator_knowledge layer (§2.3, §3.3).
- **Encryption at rest for Exile's state** (per `coo/CLAUDE.md` Product-Specific Rules). Enforced at the storage layer (§6).
- **In-flight context handling acknowledged** (per `coo/CLAUDE.md` Product-Specific Rules). Enforced at the inference assembly layer (§5.3 — prompt minimization).
- **Model version pinning** (per `coo/CLAUDE.md` Product-Specific Rules). Out of scope for this document — enforced at the inference abstraction layer.

**Additional enforcement from `EXILE.md` v0.3:**

- **The doctrinal ceiling at calibration 4b.** Per `EXILE.md` §4.A and §4.19 closing notes — the doctrine intentionally tops out at calibration 4b. Calibration 4c (fully explicit erotic detail) is out-of-doctrine. Enforced at the state model layer (§3.5 — state never reaches a value that would translate to past-4b shaping) and at the inference assembly layer (§5.2 step 5 — calibration-ceiling clamp). Defense in depth.

---

## 10. To the implementer

When this document finalizes:

1. **The four domains (§1) become four logical groupings.** They may be four SQLite databases, four schemas in one database, or four sets of tables — the logical separation is normative; the physical separation is implementation choice.

2. **The schema (§2) is the starting point for the SQLite migrations.** Field additions are expected as the project advances. Use the migration discipline in §7 from day one — even MVP's first migration should set the precedent.

3. **The state-change mechanics (§3) are the contract for the event detection layer.** This is the layer that watches conversation turns and produces rapport events, friendship-floor events, and proposed knowledge entries. It is doctrinally significant — the events it detects shape Exile's growth.

4. **The retention discipline (§4) gates context budget.** API inference at MVP makes this concrete. The summarization layer is not optional infrastructure; it is part of how Exile remembers across years.

5. **The inference assembly pipeline (§5.2) is the bridge between this spec and the inference abstraction layer.** It is also the most doctrinally consequential implementation choice — get this right and Exile's voice survives across providers and models.

6. **The encryption layer (§6) is where the Rust ramp pays off.** This is the work that's better in Rust than in Node, and one of the reasons Tauri was chosen (per `../doctrine/decisions/0011-coo-as-independent-product.md` Consequences).

7. **Migration discipline (§7) is non-optional.** The first migration sets the precedent for every migration after. Get the schema_version field on every record, the test cases for the non-negotiables in §7.3, and the additive-vs-non-additive distinction documented from MVP forward.

— end of rapport state model v0.1 —

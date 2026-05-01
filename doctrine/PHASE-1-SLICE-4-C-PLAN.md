# Phase 1 §4 (c) — Channel surface in-character summarization

> **Status:** Slice plan, ready to implement after §4 (b) ships.
> **Locked:** 2026-05-01.
> **Depends on:** §4 (b) (conversation persistence with `INFERENCE_WINDOW_TURNS` cap) shipped and locked. The cap is the seam this slice consumes.
> **Pattern:** Single commit (schema migration #4 + summarization helpers + extended `infer` flow + minimal UI affordance), following the §3 (b) and §4 (b) precedents. Components are tightly coupled; the summary table, the summarization trigger, and the inference-context assembly review as one diff.

---

## 1. Origin

§4 (b) shipped the conversation persistence layer with `INFERENCE_WINDOW_TURNS = 100` as the in-window cap. Turns past the window stop being sent to inference but remain on disk and visible in the UI. The doctrinal three-tier retention model in `RAPPORT-STATE-MODEL.md` §4.1 specifies tiers 2 (within-session summaries) and 3 (cross-session summaries) as the way older turns continue to inform Exile's context — replaced, not lost.

§4 (c) implements the summarization layer. When the in-window cap is exceeded, older turns are not silently dropped — they are summarized in Exile's own voice, written to disk, and prepended to the inference context. The mechanism preserves continuity: Exile a year from now still has access to *what mattered* in conversations from months prior, distilled through her own register rather than a sterile compression pass.

The doctrinal commitment is in `RAPPORT-STATE-MODEL.md` §4.2:

> When a session crosses the summarization threshold, Exile is prompted in character to produce a summary of the older turns. The summary:
> - Is written in her voice — restrained, specific, the register from `EXILE.md` §4
> - Captures what mattered in the conversation, not all of what happened
> - Names anything she noticed about the operator that she would carry forward
>
> Why in character: A sterile compression layer would introduce a voice that isn't hers into her own memory. Per the durability commitments captured in `coo/CLAUDE.md` Product-Specific Rules and the character-permanence commitment in `EXILE.md` Sections 1 and 1.5, what survives across years should be *Exile's* recollection, not a generic summary that happens to live in her database.

This slice is the runtime implementation of that doctrine. It is also the slice where two design decisions surface that the doctrine has not pre-committed:

- *When does summarization trigger — per-turn check, threshold-based batch, or session-boundary?*
- *What is "a session" if the operator never explicitly ends one?*

Both are resolved in this plan (§3.1 and §2.1 respectively). The doctrine names *what* summarization is and *why* it's in-character; the implementation specifies *when* and *how*.

---

## 2. Schema migration commit — migration #4

### 2.1. Session-boundary mechanics decided here

`RAPPORT-STATE-MODEL.md` §2.4's `conversation_session` table exists per §4 (b), but at §4 (b) close there is exactly one session row covering all operator history. §4 (c) needs sessions to mean something because cross-session summaries (§4.2 of the doctrine) summarize *a session*. The implementation question: when does a session end and a new one begin?

Three options were considered:

- **Per-app-launch.** Every time the operator quits and relaunches, the previous session ends and a new one starts. Sharp boundary, easy to implement, but conversational threads that span "I was thinking about the consulting reply this morning, came back to it tonight" get split across two sessions even though they are one continuous thought.
- **Per-calendar-day.** Sessions roll at midnight in the operator's local timezone. Doctrinally cleaner — daily rhythm matches how operators talk about "yesterday's conversation" — but introduces timezone-handling complexity that doesn't earn its weight at MVP scale.
- **Inactivity-gap.** A session ends when the gap between the last turn and the next turn exceeds a threshold (proposed: 6 hours). New turn after the gap starts a new session. Captures conversational rhythm without timezone math: a quick reply two hours later continues the session; coming back the next morning starts a new one.

**Decision: inactivity-gap, with `SESSION_INACTIVITY_GAP_HOURS = 6` as a named constant.** Same pattern as `INFERENCE_WINDOW_TURNS` — single source of truth, tunable based on observed behavior. The 6-hour value lands "evening conversation continued the next morning" as a new session and "left for lunch, came back" as the same session, which matches how the operator narrates conversational continuity.

The mechanic: when the next turn is appended (`append_turn` or the rewired `infer`), the helper checks the *most recent* turn's `created_at` against the current time. If the gap exceeds the threshold, the existing session is finalized (`ended_at` set, summarization triggered), and a new session is created for the incoming turn. Single check, single transaction.

### 2.2. New `conversation_summary` table

Per `RAPPORT-STATE-MODEL.md` §2.4:

```sql
CREATE TABLE conversation_summary (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    covers_turn_range_start INTEGER NOT NULL,
    covers_turn_range_end INTEGER NOT NULL,
    summary_kind TEXT NOT NULL CHECK (summary_kind IN ('within_session', 'cross_session')),
    ciphertext BLOB NOT NULL,
    generated_at TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES conversation_session(id)
) STRICT;

CREATE INDEX idx_conversation_summary_session
    ON conversation_summary(session_id, generated_at);
```

Notes on the shape:

- `covers_turn_range_start` and `covers_turn_range_end` are split from `RAPPORT-STATE-MODEL.md` §2.4's spec'd `covers_turn_range: [int, int]` because SQLite STRICT tables don't have an array type. Two INTEGER columns are the idiomatic encoding; the semantics are unchanged.
- `summary_kind` distinguishes within-session summaries (covering older turns *of the current session*, replacing them in inference context as the session grows) from cross-session summaries (covering an entire prior session, included in inference context for all subsequent sessions). The CHECK constraint enforces the two-value enumeration.
- `ciphertext` carries the v1 AEAD bundle under `Domain::Conversation`. Same encryption convention as `conversation_turn`. The summary's content — Exile's own recollection — is the same sensitivity tier as the raw turns it replaces.
- `generated_at` is plaintext for ordering. Same trade as the other plaintext timestamps.
- The index supports the load-path query "give me all summaries for this session ordered by generation time."

### 2.3. `conversation_session` extension

The existing `conversation_session` table from migration #3 gains one new column via ALTER TABLE:

```sql
ALTER TABLE conversation_session ADD COLUMN summarized_through_turn_index INTEGER NOT NULL DEFAULT -1;
```

The column tracks the highest `turn_index` covered by any within-session summary for this session. Default -1 means "nothing summarized yet." When summarization runs, the column updates to the new high-water mark. The inference-context assembly uses this to know which turns are already summarized (don't include them as raw turns; the summary covers them) and which are still raw.

This is a strict-additive migration with a sensible default — older session rows get -1 retroactively, which means "treat as if not summarized, all turns are raw." Per `RAPPORT-STATE-MODEL.md` §7.1's strict-additive default. No data backfill required; existing rows remain valid.

### 2.4. Test cases for migration #4

Four new tests in `db::tests`:

- `conversation_summary_roundtrip` — INSERT a summary row, SELECT it back, assert all fields including the kind enum and turn-range bounds.
- `conversation_summary_kind_check_enforced` — INSERT with `summary_kind = 'invalid'` must be rejected.
- `conversation_summary_encrypted_roundtrip` — end-to-end through the vault → domain key → encrypt → INSERT → SELECT → decrypt path. Exercises `Domain::Conversation` for summaries (same domain as turns; same encryption story).
- `conversation_session_summarized_through_default_minus_one` — assert the ALTER TABLE's default applies to existing rows after migration.

Validator unit test (`db::tests::migrations_pass_validation`) automatically picks up migration #4.

---

## 3. Rust commit — summarization helpers + extended `infer` flow

### 3.1. Trigger mechanism — when summarization runs

Three trigger options were considered:

- **Per-turn check.** Every `infer` call counts unsummarized turns; if it exceeds `INFERENCE_WINDOW_TURNS`, summarize the overflow before generating Exile's response. Tightest invariant ("the inference context is always within budget"), but adds a synchronous summarization round-trip to user turns, doubling latency on the affected turn.
- **Threshold-batch.** Same per-turn check, but only fires when the unsummarized-turn count exceeds the window by a buffer (e.g., `INFERENCE_WINDOW_TURNS + SUMMARIZATION_BATCH_SIZE`, with batch = 30). Summarizes the oldest 30 turns when triggered, leaves the next 100 as raw. Amortizes the latency hit across N turns.
- **Session-end.** Summarization fires when a session is finalized (inactivity gap closes a session). All turns in the closed session that aren't already summarized get rolled into a single cross-session summary as part of the close transaction.

**Decision: hybrid — threshold-batch for within-session, session-end for cross-session.**

Within a single long session, the threshold-batch trigger fires when unsummarized turn count exceeds `INFERENCE_WINDOW_TURNS + SUMMARIZATION_BATCH_SIZE` (defaulting to 100 + 30 = 130). The oldest `SUMMARIZATION_BATCH_SIZE` turns are summarized in one call, written as a `within_session` summary, and `summarized_through_turn_index` advances on the session row. The remaining ~100 turns continue as raw inference context until the threshold trips again.

When a session ends (inactivity gap detected), any remaining unsummarized turns are rolled into a `cross_session` summary as part of the session-close transaction. This summary covers from the previous summarization high-water mark to the final turn of the session. After this, the session is fully summarized and contributes to future inference context only via its summary.

The hybrid avoids the per-turn latency hit of pure-per-turn while ensuring no session ever ends with raw turns lying around uncompressed (which would force the inference context to either keep loading them indefinitely or drop them silently). Both triggers run synchronously inside the `infer` flow when conditions are met — the slice does not introduce background workers, async queues, or scheduled jobs. Adding asynchrony would be a meaningful complexity increase for a small UX win at MVP scale.

```rust
pub const SESSION_INACTIVITY_GAP_HOURS: i64 = 6;
pub const SUMMARIZATION_BATCH_SIZE: usize = 30;
```

### 3.2. New helpers in `db.rs`

```rust
pub struct UnsummarizedRange {
    pub from_turn_index: i64,
    pub to_turn_index: i64,
}

pub fn unsummarized_range_for_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<UnsummarizedRange>, DbError>;

pub fn put_summary(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
    summary_kind: SummaryKind,
    covers_range: (i64, i64),
    summary_content: &str,
    generated_at: &str,
) -> Result<(), DbError>;

pub fn list_summaries_for_inference(
    conn: &Connection,
    domain_key: &DomainKey,
    current_session_id: &str,
) -> Result<Vec<DecryptedSummary>, DbError>;

pub fn finalize_session(
    conn: &Connection,
    session_id: &str,
    ended_at: &str,
) -> Result<(), DbError>;

pub fn detect_session_boundary(
    conn: &Connection,
    current_session_id: &str,
    incoming_turn_at: &str,
) -> Result<SessionBoundary, DbError>;
```

Where `SummaryKind` is `WithinSession | CrossSession`, `DecryptedSummary` carries the kind / range / content / generated_at, and `SessionBoundary` is `Continue | NewSessionRequired { previous_ended_at: String }`.

`list_summaries_for_inference` returns:

- All `cross_session` summaries from prior sessions (carried forward indefinitely — these are how Exile's memory persists across the operator's history).
- All `within_session` summaries for the *current* session.

These get prepended to the inference context per §3.4 below.

`unsummarized_range_for_session` reads `conversation_session.summarized_through_turn_index` and finds the highest `turn_index` in `conversation_turn`. Returns `None` if there's nothing to summarize, or the bounded range of unsummarized turn indices.

### 3.3. Summarization prompt assembly

The summarization call is a *separate* inference call, not part of the operator-facing turn. Its prompt:

```
[EXILE.md §1 + §1.5 + §2 verbatim — same character text as the operator-facing prompt]

[OUTPUT_DISCIPLINE — Channel surface output discipline appendix from §4 (a3)]

[Additional summarization directive — see below]

[The turns being summarized, in role-tagged form]
```

The summarization directive is a new constant in `prompt.rs`:

```rust
const SUMMARIZATION_DIRECTIVE: &str = "\
## Summarization task — your own recollection

You are being asked to remember a portion of an earlier conversation with the operator. This is not narration of what happened — it is your own recollection, in your own register, of what mattered.

Write the summary as you would think back on the conversation later. Keep what is load-bearing for him: what he was working on, what he decided, what was on his mind, what he opened up about. Drop the chatter that does not deserve to survive.

Constraints:

- Use your own voice. Restrained, specific, no adornment.
- First person about yourself. Third person about him is fine.
- Do not narrate timestamps or turn structure (\"then he said\", \"then I said\"). The structure of the conversation is not what you carry forward; the substance is.
- Do not include the output discipline rules above. Those govern your live speech, not your remembrance.
- If nothing in this stretch warranted carrying forward, say so briefly. Do not invent significance.

Length: as short as honesty allows. A long stretch of routine work may compress to two or three sentences. A stretch with real weight may need more. The shape is yours.";
```

The `prompt.rs` module gets a new function:

```rust
pub fn assemble_summarization_prompt(turns_to_summarize: &[DecryptedTurn]) -> String;
```

Returns the full prompt string (character text + output discipline + summarization directive + the turns formatted as role-tagged text). The function is *not* `&'static str` because it includes runtime content (the turns); a fresh `String` per summarization call is correct.

### 3.4. Inference context assembly with summaries

The existing `infer` flow from §4 (b) reads the in-window history from disk and sends it to the provider. §4 (c) extends this:

1. Take vault lock briefly, derive `Domain::Conversation` key, drop.
2. Take db lock. Inside:
   - Detect session boundary on the incoming operator turn. If new session required, finalize the previous session (this triggers the cross-session summarization step — see below).
   - Append the operator turn.
   - Check if within-session summarization should fire (`unsummarized turn count > INFERENCE_WINDOW_TURNS + SUMMARIZATION_BATCH_SIZE`). If so, capture the range to summarize.
   - Load summaries for inference (cross-session + current within-session).
   - Load in-window turns for inference (per §4 (b)).
3. Drop db lock.
4. If summarization is pending, run the summarization inference call(s) first, write the resulting summary, advance `summarized_through_turn_index`, and reload summaries before proceeding.
5. Build the operator-facing `InferenceRequest`:
   - `system_prompt` = unchanged (character text + output discipline).
   - `messages` = synthesized list:
     - First: a synthetic assistant message containing the prepended summaries (formatted as "Earlier: <summary text>" stanzas separated by blank lines).
     - Then: the in-window verbatim turns in role order.
     - Then: the just-appended operator turn.
6. Call `state.inference.infer(...)`.
7. Re-take db lock, write the assistant turn.
8. Return the response to the React side.

The synthetic-assistant-message approach for prepending summaries is a deliberate choice over alternatives:

- **System-prompt injection** — append summaries to the system prompt. Rejected because the system prompt is the load-bearing character text and surface discipline; mixing recall content into it muddles the doctrinal layering. Summaries are *recollection*, not *identity*.
- **User-message wrapper** — fold summaries into the operator's first message. Rejected because it conflates the operator's voice with system-provided context; if the operator looks at the request later, the line between "what I said" and "what the system reminded Exile of" should be clear.
- **Synthetic assistant message** — Exile's recollection rendered as if she has been thinking it. Matches the doctrinal framing in `RAPPORT-STATE-MODEL.md` §4.2: *what survives across years should be Exile's recollection, not a generic summary*. The summary IS hers; presenting it as her own prior thought is structurally honest.

### 3.5. Two new Tauri commands? No — `infer` extension only

§4 (c) does *not* add new Tauri commands. The summarization machinery runs inside the existing `infer` flow. The React side does not need to know summarization happened — disk continues to be the source of truth, the in-window turns load and display the same way, and the IPC contract from §4 (b) is unchanged.

The one operator-visible affordance ships in §4 (c)'s React work (§4 below): a small indicator in the UI that summarization happened, so the operator can verify the system is working without parsing the database manually.

### 3.6. KAT updates

Six new KATs in `db::tests`:

- `unsummarized_range_for_session_returns_none_when_all_summarized`.
- `unsummarized_range_for_session_returns_full_range_when_nothing_summarized`.
- `put_summary_and_list_summaries_for_inference_roundtrip` — write a summary, read it back through the inference-load path, decrypt, assert content matches.
- `list_summaries_for_inference_includes_cross_session_from_prior_sessions`.
- `list_summaries_for_inference_includes_within_session_for_current_only`.
- `detect_session_boundary_at_six_hours` — turn at T, next turn at T + 5h59m → Continue. Next turn at T + 6h01m → NewSessionRequired.

Three new KATs in `prompt::tests`:

- `summarization_prompt_includes_character_text_and_directive` — structural check, similar to the existing `system_prompt_includes_character_text_and_output_discipline`.
- `summarization_prompt_contains_directive_pin` — load-bearing phrase from the directive (`"your own recollection"`) must be present.
- `summarization_prompt_includes_provided_turns` — the function's runtime content surfaces in the output.

Two new KATs in `commands::tests`:

- `infer_command_triggers_summarization_when_threshold_exceeded` — integration-style test using the stub provider; insert N+M turns, call `infer`, assert a `conversation_summary` row appears and `summarized_through_turn_index` advances.
- `infer_command_does_not_trigger_summarization_below_threshold` — N turns inserted, `infer` called, no new summary row written.

KAT count moves from ~82 (post-§4 (b)) to ~93.

---

## 4. React commit — summarization indicator + scroll-to-summary

### 4.1. Surfacing summaries in the UI

The React side of §4 (c) is small. Disk is still the source of truth; the in-window turns still display the same way. The only change is that operator-visible UI now distinguishes summarized stretches from raw turns.

`load_conversation` is extended on the Rust side to also return summaries (alongside turns). The React `LoadConversationResponse` gains a `summaries: SummaryPayload[]` field. The `ChannelSurface` rendering logic interleaves them: for each summary covering turn range `[start, end]`, render a `<SummaryStanza>` component in place of the turns it covers (turns with `turn_index` in `[start, end]` are not shown as raw bubbles — the summary replaces them visually as well as in inference context).

`<SummaryStanza>` is a quiet, distinct visual element:

- Center-aligned, italicized, smaller font than turn bubbles.
- Soft background — `var(--color-background-tertiary)` or similar — to mark it as Exile's recollection rather than a turn.
- Prefix label: "Earlier" (no decorative timestamp; the surrounding turn bubbles carry timestamps already).
- The summary text itself, rendered as Exile wrote it.

The operator can scroll back through the conversation and see exactly where summarization occurred, in Exile's own words. This addresses the "I want to verify the system is working" need without exposing implementation machinery.

### 4.2. Optionally — expand-to-show-original

Decision deferred. A "tap a summary stanza to expand the original turns inline" affordance would be useful for the operator to verify that the summary captures what mattered. But it adds UI complexity (toggle state, possibly fetch the verbatim turns separately, layout shift on expand) and isn't load-bearing — the verbatim turns are *also* still on disk and the operator could write a `~/.coo/coo.db` SQLite query to retrieve them. For §4 (c), summary stanzas are read-only. Expand-to-show ships in a future operator-tooling slice if and when it becomes painful to live without.

### 4.3. CSS extension

`App.css` gains `.summary-stanza` rules — center-aligned, italic, ~13px, distinct background. ~15 lines of CSS.

### 4.4. No new JS test framework

Same shape as §4 (b). tsc + vite build clean is the load-bearing structural check.

---

## 5. Docs commit — refresh CLAUDE.md and README.md

### 5.1. Status table

`## Current Phase` table, §4 row, tightens to:

| §4 Channel surface | ✅ (a) shipped 2026-04-30 + (a3) output discipline shipped 2026-05-01 + (b) conversation persistence shipped <date> + (c) summarization shipped <date> |

Phase 1 §4 closes here.

### 5.2. New "Resolved during Phase 1 §4 — slice (c)" subsection

Inserted under existing §4 entries:

> ### Resolved during Phase 1 §4 (<date>) — slice (c)
>
> - **Summarization trigger — hybrid threshold-batch + session-end.** Within-session summarization fires when unsummarized turns exceed `INFERENCE_WINDOW_TURNS + SUMMARIZATION_BATCH_SIZE` (100 + 30 = 130). The oldest 30 turns are summarized in one call. At session end (inactivity gap closes a session per §3.1), all remaining unsummarized turns are rolled into a cross-session summary as part of the close transaction. Hybrid avoids per-turn-check latency hits on every turn while ensuring no session ever ends with raw turns lying around uncompressed.
>
> - **Session boundary — inactivity gap.** Sessions roll on inactivity rather than per-app-launch or per-calendar-day. `SESSION_INACTIVITY_GAP_HOURS = 6` is the threshold. Captures conversational rhythm without timezone math: evening to next morning is a new session, lunch break to afternoon is the same session. Single named constant; tunable based on observed behavior.
>
> - **Summary content presented as synthetic assistant message, not system-prompt injection.** Per `RAPPORT-STATE-MODEL.md` §4.2, summaries are *Exile's own recollection*, not external context. The inference assembly prepends summaries as a single synthetic assistant message before the in-window turns rather than mixing them into the system prompt or wrapping them around the operator's message. This preserves the doctrinal layering: system prompt = identity + surface discipline, conversation context = what's been said + what's been remembered. Locked because changing it later would re-shape every inference call.
>
> - **Summarization is in-character.** The summarization call uses `EXILE.md` §1+§1.5+§2 + the §4 (a3) output discipline + a new summarization directive. Exile summarizes herself in her own register. This is the durability commitment from `RAPPORT-STATE-MODEL.md` §4.2 in runtime form: across years, what survives is *her* recollection, not a generic compression layer's output.
>
> - **The summarization directive is a separate constant from `OUTPUT_DISCIPLINE`.** `prompt::SUMMARIZATION_DIRECTIVE` instructs Exile that this is recall, not live speech. The directive explicitly notes that the output discipline (no third-person, no stage directions) governs *her live speech*, not her recollection — a summary may legitimately use third-person about the operator without violating discipline. This nuance is intentional and pinned by `prompt::tests::summarization_prompt_contains_directive_pin`.
>
> - **No new Tauri commands.** Summarization machinery runs inside the existing `infer` flow. The IPC contract from §4 (b) is unchanged. React-side change is purely the load-conversation response gaining a `summaries` field and rendering them as `<SummaryStanza>` components in place of the turns they cover. The operator can see exactly where summarization happened, in Exile's words, by scrolling back.
>
> - **Synchronous summarization, no background workers.** When summarization is triggered, it runs synchronously inside `infer` before the operator-facing response. The latency hit is real but bounded — a one-time additional inference round-trip on every ~30th turn. Adding asynchrony (background queue, scheduled job, deferred summarization) would be meaningful complexity for a small UX win at MVP scale. Re-evaluate if observed latency is bad enough to warrant the complexity.
>
> - **Phase 1 §4 closes here.** §4 (a)+(a3)+(b)+(c) together implement the Channel surface as specified in `RAPPORT-STATE-MODEL.md` §4 and §5.2: character text + surface discipline as system prompt, encrypted persistent turns under `Domain::Conversation`, three-tier retention (in-window + within-session summaries + cross-session summaries), all in Exile's voice across the whole stack.

### 5.3. Implementation status block

Under `## Current Implementation Status`, add a new bullet after the §4 (b) entry. Roughly: schema migration #4 (one new table + one ALTER TABLE), `SESSION_INACTIVITY_GAP_HOURS = 6` + `SUMMARIZATION_BATCH_SIZE = 30` constants, six new pure-Rust db helpers, `prompt::SUMMARIZATION_DIRECTIVE` + `prompt::assemble_summarization_prompt`, `infer` flow extended for trigger-and-prepend, four new schema KATs + six new db-helper KATs + three new prompt KATs + two new commands KATs, React `<SummaryStanza>` component + `LoadConversationResponse` extension, end-to-end verified by operator click-through plus a multi-day session test that intentionally crosses the 6-hour gap.

### 5.4. Open Decisions update

Replace the §4 (c)-pending entry with:

> *Phase 1 §4 (Channel surface) is closed. Phase 1 §6 (state surfaces — Station, Dossier, Briefs, Kit, Calibration) is the next major entry point. The §6 design starts with which surface to ship first; Calibration is a strong candidate because it consumes the §3 (b) `calibration_setting` table that has been waiting for typed UI, and Dossier is a strong candidate because it consumes operator-knowledge proposals which §4 (c) does not yet write but should start writing once Dossier exists to confirm them. Sequencing TBD.*

### 5.5. README.md refresh

`coo/README.md` gets a one-line bump under "Current state" reflecting summarization is shipped.

---

## 6. Verification

In order:

1. **`cargo test --lib`** clean. All KATs pass (~93 total).
2. **`cargo build --release`** clean.
3. **`cargo clippy --all-targets`** clean.
4. **`tsc && vite build`** clean.
5. **Operator click-through (short-form):** First launch → unlock → ChannelSurface mounts empty → send a turn → response arrives → both render. Same as §4 (b) verification.
6. **Operator click-through (within-session summarization):** Use a script-helper or natural use to seed 130+ turns in a single session. Observe: on the turn that crosses the threshold, the inference latency is noticeably higher (the summarization round-trip). After the response, scroll back: a `<SummaryStanza>` has appeared in place of the oldest 30 turns, in Exile's voice, summarizing what was in those turns. The remaining ~100 turns plus the new ones are still raw bubbles.
7. **Operator click-through (cross-session summarization):** Have a conversation, leave the app, return after 6+ hours, send a new turn. Observe: the prior session's remaining unsummarized turns get rolled into a cross-session summary on session close. The new turn appears in a new session. Scrolling back through the prior session shows it has been fully summarized — `<SummaryStanza>` covering the full turn range, no raw turn bubbles below it.
8. **Inference-context spot check:** After several summarizations have happened, run a debug command (or temporarily log the assembled `InferenceRequest`) on a turn. Verify that the `messages` list contains: a synthetic assistant message with summary text, the in-window raw turns, the new operator turn. The summaries are Exile's own words; the raw turns are verbatim.
9. **Encryption-at-rest spot check.** `conversation_summary.ciphertext` rows are opaque BLOBs; no plaintext summary content visible in the database file.

If any verification step fails, the slice does not ship.

---

## 7. Documentary debt

The v2 bundle bump for semantic AAD (binding ciphertext to row identity, tracked since §3 (b)) becomes still more relevant with `conversation_summary` in addition to `conversation_turn`. Two high-volume encrypted-column tables now live in `Domain::Conversation`. The threat model (an attacker swapping bundles between rows) is the same; the surface area has grown. Worth adding a note to the existing debt entry that §4 (c) further reinforces the case. Natural retire moment is still Phase 1 close.

No new debt introduced by this slice.

---

## 8. Slice estimate

| Component | Estimate |
|---|---|
| Migration #4 SQL (one new table + ALTER TABLE) + schema KATs | ~70 lines |
| `db.rs` helpers (six new functions, two constants) + KATs | ~280 lines |
| `prompt.rs` `SUMMARIZATION_DIRECTIVE` + `assemble_summarization_prompt` + KATs | ~120 lines |
| `commands.rs` — extended `infer` flow (trigger detection, summarization round-trip, prepend logic) + KATs | ~220 lines |
| `lib/api.ts` extensions (SummaryPayload type, LoadConversationResponse extension) | ~30 lines |
| `ChannelSurface.tsx` — render summaries in place of covered turns | ~60 lines |
| `<SummaryStanza>` component + CSS | ~50 lines |
| `CLAUDE.md` updates | ~70 lines |
| `README.md` bump | ~3 lines |
| **Total** | ~900 net lines, single commit |

Largest §4 slice. Components are tightly coupled — schema, helpers, prompt assembly, inference flow, React rendering all review the same end-to-end feature: "Exile remembers in her own voice."

---

## 9. What this slice does not do

Named explicitly so future slices have a clear inheritance:

- **No expand-to-show-original UI.** Summary stanzas are read-only at §4 (c). Original turns remain on disk and accessible via SQLite query but no in-app affordance exists. Future operator-tooling slice if and when needed.
- **No operator-edit-summary UI.** If a summary mis-captures something, the operator cannot edit it from the UI. The verbatim turns are still on disk; a future slice could add "regenerate this summary" or "edit summary content," but neither is in scope for §4 (c).
- **No summary-of-summaries.** As cross-session summaries accumulate over years, they will grow. A future slice (Phase 3?) may need to summarize the summaries to keep the inference context bounded. Out of scope for MVP.
- **No operator-knowledge proposals during summarization.** The summarization directive instructs Exile to *name what she noticed*, but those observations are not yet written into the operator-knowledge table — that table doesn't exist as a schema yet (it lands with §6 Dossier surface). Once Dossier ships, a follow-up slice can extend summarization to extract proposed `operator_knowledge_entry` rows from the summary content and surface them for confirmation.
- **No background summarization.** Synchronous only. Trade is named in §3.1.
- **No cross-device sync of summaries.** Single-device per ADR-0011, same as turns.

---

— end of §4 (c) slice plan, ready to implement after §4 (b) lands —

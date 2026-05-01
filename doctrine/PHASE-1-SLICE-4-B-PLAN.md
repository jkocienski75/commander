# Phase 1 §4 (b) — Channel surface conversation persistence

> **Status:** Slice plan, ready to implement after §4 (a3) ships.
> **Locked:** 2026-05-01.
> **Depends on:** §4 (a3) (output discipline) shipped and locked. §4 (a3) is not technically a blocker, but landing this slice on top of an in-flight (a3) creates merge friction in `prompt.rs` and `commands.rs` that is cheap to avoid by sequencing.
> **Pattern:** Single commit (schema migration + db helpers + IPC commands + React load-on-mount), following the §3 (b) precedent. The components are tightly coupled and review as one diff.

---

## 1. Origin

§4 (a) shipped 2026-04-30 across (a1) Rust IPC + (a2) React `ChannelSurface`. The component holds `messages: Message[]` in `useState`. App restart → empty conversation. End-to-end Exile-on-screen is working but the thread does not survive.

§4 (b) lands the conversation persistence layer. Turns are written to disk on every round-trip, encrypted under `Domain::Conversation`, loaded on `ChannelSurface` mount. The thread becomes a thing that exists rather than a session that vanishes when the app closes.

Two design questions surfaced during slice design (2026-05-01) and were resolved before this plan was written:

- *Does sending unbounded conversation history to inference risk hallucination or character drift?* Yes — and the doctrine has thought about it. `RAPPORT-STATE-MODEL.md` §4.1 specifies a three-tier retention model (in-window verbatim → summarized within-session → archived cross-session summaries). §4 (b) ships **tier 1 only** — the in-window cap. Tiers 2 and 3 (summarization, archive) ship in §4 (c).
- *Does the agent eventually reference enriched entities for context rather than raw turn history?* Yes, and that's the structural answer to the hallucination problem. Operator-knowledge entries (per `RAPPORT-STATE-MODEL.md` §1.3 and §2.3) carry forward what matters about the operator across summarization. §4 (b) does not implement operator-knowledge — that lands in §6 (Dossier surface). §4 (b) is the substrate that summarization in §4 (c) and operator-knowledge in §6 will both build on.

The tier-1 cap is what differentiates this slice from a pure-substrate ship. Without it, a §4 (b) that loads everything and sends everything would contradict the doctrinal retention model and force §4 (c) to add both summarization *and* the cap simultaneously. With it, §4 (c)'s job becomes the focused one: replace the dropped-out-of-window older turns with in-character summaries. Same seam, two slices, each with one job.

---

## 2. Schema migration commit — migration #3

### 2.1. Two new tables

Per `RAPPORT-STATE-MODEL.md` §2.4:

```sql
CREATE TABLE conversation_session (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    turn_count INTEGER NOT NULL DEFAULT 0,
    schema_version INTEGER NOT NULL
) STRICT;

CREATE TABLE conversation_turn (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_index INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    ciphertext BLOB NOT NULL,
    created_at TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES conversation_session(id),
    UNIQUE (session_id, turn_index)
) STRICT;

CREATE INDEX idx_conversation_turn_session_index
    ON conversation_turn(session_id, turn_index);
```

Notes on the shape:

- `id` columns are TEXT to hold UUIDs (string-rendered). Matches the precedent set by `operator_knowledge_entry.id` in `RAPPORT-STATE-MODEL.md` §2.3.
- `role` is a plaintext CHECK-constrained TEXT column rather than encrypted. The role distinction (user vs. assistant) is structural — it shapes the role of each turn in the inference request — and not sensitive on its own. Encrypting it would mean decrypting on every read just to know which speaker; the cost is real, the protection is nil.
- `ciphertext` carries the §2 (c) v1 AEAD bundle (6-byte header + 24-byte nonce + ct+tag) under `Domain::Conversation`, as established in §3 (b) for the other encrypted-column tables.
- `created_at` is plaintext for ordering and for the date-divider rendering on the React side. Leaks "operator was active at time T" but no content. Same trade as `app_config.updated_at` and `operator_profile.updated_at` in §3 (b).
- `schema_version` is required-explicit (no DEFAULT) per `RAPPORT-STATE-MODEL.md` §7.2's versioned-envelope discipline. New rows commit to their own version.
- The `UNIQUE (session_id, turn_index)` constraint catches double-writes if a future bug or retry path tries to insert the same turn twice. The index on `(session_id, turn_index)` makes the load-path SELECT cheap regardless of total turn volume.
- No `conversation_summary` table in this migration. Per `RAPPORT-STATE-MODEL.md` §2.4 the summary table is part of the retention model, but it does not get exercised until §4 (c) implements summarization. Migrating it now would be pre-committing schema shape that hasn't been tested against real conversation flow. §4 (c) will add migration #4 with the summary table when it's actually needed.

### 2.2. Test cases for the migration

Three new tests in `db::tests`, mirroring the §3 (b) pattern:

- `conversation_session_roundtrip` — INSERT a session row, SELECT it back, assert all fields including `started_at`, `ended_at`, `turn_count`.
- `conversation_turn_unique_session_index_enforced` — two INSERTs with the same `(session_id, turn_index)` — second must be rejected by the UNIQUE constraint.
- `conversation_turn_encrypted_roundtrip` — end-to-end through `vault::setup_passphrase` → `domain_key(Conversation)` → `encrypt(content)` → INSERT → SELECT → `decrypt` → assert the plaintext matches. Mirrors `operator_profile_encrypted_roundtrip` and `calibration_setting_encrypted_roundtrip` from §3 (b), exercising the encrypted-column convention against `Domain::Conversation` for the first time.

Validator unit test (`db::tests::migrations_pass_validation`) automatically picks up the new migration.

---

## 3. Rust commit — db helpers + IPC commands

### 3.1. Three new pure-Rust helpers in `db.rs`

Following the §3 (c1) precedent — pure-Rust write helpers separated from `#[tauri::command]` wrappers so they remain unit-testable without a Tauri runtime.

```rust
/// Maximum recent turns sent to inference per call. Older turns remain on disk
/// and are loaded by `list_turns_for_ui` for display, but `list_turns_for_inference`
/// caps at this value.
///
/// This is the in-window tier of RAPPORT-STATE-MODEL.md §4.1's three-tier retention
/// model. Tiers 2 and 3 (summarization, archive) ship in §4 (c) and replace
/// dropped turns with in-character summaries. Until then, turns past the window
/// are simply not sent to inference — they remain on disk and visible in the UI.
pub const INFERENCE_WINDOW_TURNS: usize = 100;

pub fn create_session(conn: &Connection, id: &str, started_at: &str) -> Result<(), DbError>;

pub fn put_turn(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
    turn_index: i64,
    role: TurnRole,
    content: &str,
    created_at: &str,
) -> Result<(), DbError>;

pub fn list_turns_for_ui(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
) -> Result<Vec<DecryptedTurn>, DbError>;

pub fn list_turns_for_inference(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
) -> Result<Vec<DecryptedTurn>, DbError>;
```

Where `TurnRole` is a `User | Assistant` enum (mirrors `inference::Role` but distinct so the db layer doesn't depend on the inference module — the dependency direction is one-way: inference → db is fine, db → inference would be a layering inversion), and `DecryptedTurn` is `{ turn_index: i64, role: TurnRole, content: String, created_at: String }`.

The two `list_turns_*` variants differ only in their query:

- `list_turns_for_ui` returns *all* turns for the session, ascending by `turn_index`. Used by the React load-on-mount path. The operator can scroll back to anything they have ever said.
- `list_turns_for_inference` returns the *most recent N* (`INFERENCE_WINDOW_TURNS`), ascending by `turn_index`. Used by the `infer` command. SQL: `SELECT ... ORDER BY turn_index DESC LIMIT ? ` then reverse in Rust to ascending order. The reverse-and-LIMIT pattern is the standard SQLite idiom for "last N rows."

The two functions are deliberately separate rather than parameterized as `list_turns(conn, key, session_id, limit: Option<usize>)`. Reason: callers should not be making the choice at call sites. The UI load path always wants everything; the inference path always wants the window. Naming the two paths clearly is cheaper than naming the parameter clearly.

### 3.2. Five new tests in `db::tests`

- `create_session_and_put_turn` — happy path, both functions, assert the row appears with correct fields.
- `put_turn_rejects_duplicate_session_index` — UNIQUE constraint test from migration validation, exercised through the public API.
- `list_turns_for_ui_returns_all_in_order` — insert turns out of order, assert SELECT returns them ascending.
- `list_turns_for_inference_caps_at_window` — insert N+50 turns, assert exactly `INFERENCE_WINDOW_TURNS` come back, and that they are the *most recent* N (turn_index values N..N+50, not 0..N).
- `list_turns_for_inference_returns_ascending` — turns must come back ascending even though the SQL fetched them descending. Catches a regression where the reverse step gets dropped in a future refactor.

### 3.3. Two new Tauri commands in `commands.rs`

```rust
#[tauri::command]
pub async fn load_conversation(
    state: State<'_, AppState>,
) -> Result<LoadConversationResponse, ConversationCommandError>;

#[tauri::command]
pub async fn append_turn(
    session_id: String,
    role: TurnRole,
    content: String,
    state: State<'_, AppState>,
) -> Result<AppendTurnResponse, ConversationCommandError>;
```

`load_conversation` returns:

```rust
#[derive(Serialize)]
pub struct LoadConversationResponse {
    session_id: String,
    turns: Vec<TurnPayload>,  // for the UI — ALL turns
}
```

It also handles the "is there a current session?" decision: if no `conversation_session` rows exist, it creates one with a fresh UUID and the current timestamp before returning. If multiple sessions exist (which won't happen in §4 (b), but will once §4 (c) adds session boundaries), it returns the most recent. This keeps the React side dumb — it asks for the conversation, it gets the conversation.

`append_turn` is the write side. It inserts the turn under the existing session (creating one if somehow absent), increments the session's `turn_count`, and returns:

```rust
#[derive(Serialize)]
pub struct AppendTurnResponse {
    turn_index: i64,
    created_at: String,
}
```

The two writes (turn INSERT + session turn_count UPDATE) are wrapped in a single SQLite transaction. Atomicity matters here — half a write would leave `turn_count` out of sync with the actual rows.

`ConversationCommandError` is a JSON-tagged enum following the §4 (a1) `InferenceCommandError` precedent:

```rust
#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationCommandError {
    VaultLocked,
    Db { message: String },
    Crypto { message: String },
}
```

JS can pattern-match on `kind` and render distinct UI per variant. `VaultLocked` is the case where `state.vault` is `None` — the operator hasn't unlocked yet. The UI should not allow this in normal flow (App.tsx routes through UnlockScreen first), but the variant exists for defense in depth.

### 3.4. `infer` command extended to read from disk

The existing `infer` command in `commands.rs` (per §4 (a1)) takes `messages: Vec<inference::Message>` from the JS side. §4 (b) extends it to read the in-window history from disk *before* calling the provider, so the React side does not need to send the full message list every turn.

New shape:

```rust
#[tauri::command]
pub async fn infer(
    session_id: String,
    operator_turn: String,
    state: State<'_, AppState>,
) -> Result<InferResponse, InferenceCommandError>;
```

Where `InferResponse` is:

```rust
#[derive(Serialize)]
pub struct InferResponse {
    assistant_content: String,
    turn_indices: TurnIndices,  // { user: i64, assistant: i64 }
    created_at: TurnTimestamps, // { user: String, assistant: String }
}
```

The flow inside `infer`:

1. Take vault lock, derive `Domain::Conversation` key, drop vault lock (per the §3 (c1) lock-ordering convention).
2. Take db lock. Inside the lock:
   - Append the operator turn via `put_turn`. Capture the `turn_index` and `created_at`.
   - Call `list_turns_for_inference` to get the in-window history (now including the just-written operator turn).
3. Drop db lock.
4. Build the `InferenceRequest` from the in-window history.
5. Call `state.inference.infer(...)`. (Network round-trip, no locks held.)
6. Re-take vault lock to derive the conversation key again, drop. Re-take db lock to write the assistant turn.
7. Return both turn indices and timestamps to the React side.

The JS side no longer maintains a `messages: Message[]` list as the source of truth — disk is the source of truth, the JS-side state is a *projection* of disk state, and `infer` keeps disk authoritative.

This is a meaningful architectural shift from §4 (a1)'s shape: the IPC contract changes. The §4 (a1) `infer` command shipped two months ago and has one consumer (`ChannelSurface`'s `dispatch` helper in §4 (a2)). Re-shaping it now is cheap; re-shaping it after §6 surfaces have built on it would not be. **This is the load-bearing reason §4 (b) lands as a single coupled commit rather than substrate-then-rewire.**

### 3.5. KAT updates for the rewired `infer` command

The §4 (a1) `commands::tests::inference_command_error_wire_shape_is_pinned` KAT continues to pass — `InferenceCommandError` is unchanged.

Two new KATs:

- `commands::tests::infer_response_wire_shape_is_pinned` — locks the new `InferResponse` JSON shape (assistant_content, turn_indices.user, turn_indices.assistant, created_at.user, created_at.assistant). The React side depends on this shape; drift breaks the channel.
- `commands::tests::conversation_command_error_wire_shape_is_pinned` — locks `ConversationCommandError`'s three variants and their JSON tags. Same purpose, new error type.

The §4 (a1) `prompt::tests::*` KATs continue to pass unchanged — the system prompt assembly is not touched.

---

## 4. React commit — load-on-mount + date dividers + IPC rewire

### 4.1. New `lib/api.ts` exports

```typescript
export type TurnRole = 'user' | 'assistant';

export type TurnPayload = {
    turn_index: number;
    role: TurnRole;
    content: string;
    created_at: string;  // ISO 8601
};

export type LoadConversationResponse = {
    session_id: string;
    turns: TurnPayload[];
};

export type ConversationCommandError =
    | { kind: 'vault_locked' }
    | { kind: 'db'; message: string }
    | { kind: 'crypto'; message: string };

export type InferResponse = {
    assistant_content: string;
    turn_indices: { user: number; assistant: number };
    created_at: { user: string; assistant: string };
};

export async function loadConversation(): Promise<LoadConversationResponse>;
export async function infer(sessionId: string, operatorTurn: string): Promise<InferResponse>;
```

The existing `infer(messages)` signature from §4 (a2) is replaced. No backwards-compat shim — per CLAUDE.md "avoid backwards-compatibility hacks." The single consumer (`ChannelSurface`) updates in this same commit.

### 4.2. `ChannelSurface` rewire

The component's local-state model changes meaningfully:

```typescript
// Before (§4 (a2)):
const [messages, setMessages] = useState<Message[]>([]);

// After (§4 (b)):
const [sessionId, setSessionId] = useState<string | null>(null);
const [messages, setMessages] = useState<TurnPayload[]>([]);
const [loadError, setLoadError] = useState<ConversationCommandError | null>(null);
```

`useEffect` on mount calls `loadConversation()`, populates `sessionId` and `messages`. On error, populates `loadError`. The empty-state copy ("The Channel is open. Speak when you're ready.") still renders when `messages` is empty post-load, so a new session looks identical to the §4 (a) experience.

`dispatch(operatorContent)` becomes:

1. Optimistically append a `TurnPayload` for the operator turn with `turn_index = -1` and `created_at = new Date().toISOString()` so the bubble appears immediately.
2. Call `infer(sessionId, operatorContent)`.
3. On success: replace the optimistic `-1` turn with the real `turn_indices.user` + `created_at.user`, append the assistant turn from the response.
4. On error: keep the optimistic operator turn (don't make the operator retype) and show the existing `InferenceCommandError` retry UI.

The optimistic UI keeps the typing-feels-instant property §4 (a2) had, while making disk authoritative.

### 4.3. Date dividers

A pure rendering helper, no state:

```typescript
function renderTurnsWithDividers(turns: TurnPayload[]): React.ReactNode[] {
    // Walk turns in order, inserting <DateDivider> between turns whose
    // calendar day differs (in the operator's local timezone).
    // Format: "Today, 7:14 AM" / "Yesterday, 9:42 PM" / "Apr 15, 2:00 PM"
}
```

Uses `Intl.DateTimeFormat` for locale-correct rendering. The "Today" / "Yesterday" determination is relative to "now at render time" — a turn from 11:59 PM yesterday will read as "Yesterday" until midnight, then re-read as "Apr 30" on the next render after midnight. This is a re-render concern, handled by re-running `renderTurnsWithDividers` on every state change (cheap — it walks the turn list once with O(N) cost, and N is bounded by the operator's actual conversation history which is small enough that a freshly-computed string array per render is fine for the UX it serves).

### 4.4. CSS extension

`App.css` gains a `.date-divider` rule — center-aligned, small, muted text, ~12px above and below for breathing room. ~10 lines of CSS. No layout reflow.

### 4.5. Verification

Same shape as §3 (c2) and §4 (a2) — no JS test framework added in this slice. tsc + vite build clean is the load-bearing structural check (TypeScript catches drift between Rust-side wire shapes and the React `switch` statements). Operator click-through verifies the experience:

- Launch app → unlock → ChannelSurface mounts → empty (first launch) or populated (subsequent launches).
- Send a turn → optimistic bubble appears immediately → assistant response arrives → both render.
- Quit app → relaunch → unlock → same conversation visible, same scroll position acceptable to land at the bottom.
- Send 100+ turns over a session, observe inference still working. (Cap is at 100 turns sent to inference; the operator may not notice the cap directly until conversations cross that boundary, which is the point — §4 (c) replaces dropped turns with summaries before the operator notices their absence.)

---

## 5. Docs commit — refresh CLAUDE.md and README.md

### 5.1. Status table

`## Current Phase` table, §4 row, tightens to:

| §4 Channel surface | ✅ (a) shipped 2026-04-30 + (a3) output discipline shipped 2026-05-01 + (b) conversation persistence shipped <date>; (c) summarization pending |

### 5.2. New "Resolved during Phase 1 §4 — slice (b)" subsection

Inserted under existing §4 entries:

> ### Resolved during Phase 1 §4 (<date>) — slice (b)
>
> - **In-window cap (`INFERENCE_WINDOW_TURNS = 100`) ships with the substrate.** §4 (b) implements tier 1 of `RAPPORT-STATE-MODEL.md` §4.1's three-tier retention model. Tiers 2 (summarization) and 3 (archived cross-session summaries) ship in §4 (c). The cap is in `db::INFERENCE_WINDOW_TURNS` as a named constant, single source of truth. Operator-visible: turns past the window are not sent to inference but remain on disk and visible in the UI. Doctrinally aligned with the retention model rather than substrate-only-ship-then-fix-later. The §4 (c) job is replacing the dropped turns with in-character summaries — same seam, focused job.
>
> - **Two `list_turns_*` functions rather than parameterized.** `list_turns_for_ui` returns all turns; `list_turns_for_inference` returns the most recent `INFERENCE_WINDOW_TURNS`. Callers don't make the choice at call sites — the UI load path always wants everything, the inference path always wants the window. Naming the paths clearly is cheaper than naming a parameter clearly.
>
> - **`infer` command IPC contract changed.** §4 (a1) shipped `infer(messages: Vec<Message>) -> String`; §4 (b) ships `infer(session_id, operator_turn) -> InferResponse` with the message history read from disk inside the command. Disk becomes the source of truth; React state is a projection. The §4 (a1) shape had one consumer (`ChannelSurface.dispatch`) and re-shaping it now is cheap; re-shaping after §6 surfaces had built on it would not be. Locked by new `commands::tests::infer_response_wire_shape_is_pinned`.
>
> - **No `conversation_summary` table in migration #3.** `RAPPORT-STATE-MODEL.md` §2.4 specifies the summary table as part of the retention model. §4 (b) does not exercise it — summaries are not written until §4 (c) implements summarization. Migrating the table now would pre-commit schema shape that hasn't been tested against real conversation flow. §4 (c) will add migration #4 with the summary table when it's actually needed. Same restraint as §3 (b) deferring calibration typing to §6.
>
> - **`role` is plaintext, content is encrypted.** `conversation_turn.role` is a CHECK-constrained TEXT column rather than encrypted. The role is structural (it shapes the inference request), not sensitive. Decrypting on every read just to know which speaker would cost real and protect nothing. Same logic as §3 (b)'s `app_config.key` and `calibration_setting.dial_key` being plaintext.
>
> - **Lock ordering preserved across the rewired `infer` command.** Vault before db, never simultaneous. The `infer` command takes vault lock briefly to derive the domain key, drops it, takes db lock to write the operator turn and read the in-window history, drops it, makes the network call without holding any locks, then re-takes vault and db locks to write the assistant turn. Same convention as §3 (c1); applies cleanly to the longer flow.
>
> - **Optimistic operator-turn rendering.** The React side appends the operator's turn to local state before the IPC round-trip completes (with `turn_index = -1`), so the bubble appears at typing latency. On `infer` success, the optimistic turn is replaced with the real one carrying the disk-assigned turn_index. On error, the optimistic turn is preserved (operator doesn't retype) and the existing retry UI handles the recovery. Keeps the §4 (a2) instant-feedback property while making disk authoritative.

### 5.3. Implementation status block

Under `## Current Implementation Status`, add a new bullet after the §4 (a3) entry. Roughly: schema migration #3, two new tables + index, `INFERENCE_WINDOW_TURNS = 100` constant, three new pure-Rust db helpers, two new Tauri commands, `infer` command rewired to read disk, three new schema KATs + five new db KATs + two new wire-shape KATs, React `ChannelSurface` rewired with optimistic operator turn + date dividers, end-to-end verified by operator click-through.

### 5.4. Open Decisions update

Replace the placeholder "no design questions currently blocking work" entry with:

> *No design questions currently blocking work. Phase 1 §4 (c) — summarization on top of the §4 (b) substrate — is the next entry point. The cap at `INFERENCE_WINDOW_TURNS` is the seam; §4 (c) replaces dropped turns with in-character summaries per `RAPPORT-STATE-MODEL.md` §4.2.*

### 5.5. README.md refresh

`coo/README.md` gets a one-line bump under "Current state" reflecting persistence is shipped. The README is the operator-facing what's-working summary; CLAUDE.md is the implementation truth.

---

## 6. Verification

In order:

1. **`cargo test --lib`** clean. All KATs pass:
   - 3 new schema KATs in `db::tests` (session roundtrip, unique constraint, encrypted-turn roundtrip).
   - 5 new db-helper KATs (create_session+put_turn, duplicate rejection, list-for-ui ordering, list-for-inference cap, list-for-inference ascending after reverse).
   - 2 new wire-shape KATs (`infer_response`, `conversation_command_error`).
   - All §3 / §4 (a) / §5 KATs pass unchanged.
   - Total goes from 72 (post-§4 (a3)) to ~82.
2. **`cargo build --release`** clean.
3. **`cargo clippy --all-targets`** clean.
4. **`tsc && vite build`** clean. TypeScript catches drift between the new Rust-side wire shapes and the React `switch` statements.
5. **Operator click-through:**
   - First launch: `ChannelSurface` mounts empty, send a turn, response arrives, both render with date divider above the first turn.
   - Quit, relaunch, unlock: same conversation visible. Scroll position lands at bottom.
   - Across-day test: leave a turn at end of day; next morning, send a new turn; observe the "Yesterday" divider correctly placed above the prior day's turns.
   - Cap test: send 100+ turns in one session (or run a script-helper to seed turns); confirm inference still works and recent turns are present in the response context.
6. **Encryption-at-rest spot check.** After conversation has happened, open `~/.coo/coo.db` in a SQLite viewer. `conversation_turn.ciphertext` rows should be opaque BLOBs, no plaintext content visible. `role` and `created_at` are plaintext as expected.

If any verification step fails, the slice does not ship — schema migration, db helpers, IPC commands, and React rewire land together or not at all.

---

## 7. Documentary debt — none new introduced

Tracked debts (`RAPPORT-STATE-MODEL.md` §6.6 envelope-crate, in-memory hygiene, lock-key/unlock-translation, doctrine bundle move, v2 bundle bump for semantic AAD) are unchanged by this slice.

The v2 bundle bump (semantic AAD binding ciphertext to row identity) becomes more relevant with `conversation_turn` because it's the highest-volume encrypted-column table. An attacker with write access to the SQLite file could swap a valid bundle from one turn into another turn's BLOB column; the AEAD would accept it. Not a primary concern (defense in depth on top of OS-level file protections), but worth a note in the debt entry: §4 (b) is the slice that makes the v2 case strongest. Natural retire moment is still Phase 1 close — the design once, applied uniformly across all four encrypted-column tables (`operator_profile`, `calibration_setting`, `conversation_turn`, plus whatever lands in §6).

---

## 8. Slice estimate

| Component | Estimate |
|---|---|
| Migration #3 SQL + schema KATs | ~80 lines |
| `db.rs` helpers (`create_session`, `put_turn`, `list_turns_*`, `INFERENCE_WINDOW_TURNS`) + KATs | ~180 lines |
| `commands.rs` — `load_conversation`, `append_turn`, rewired `infer`, error type, KATs | ~200 lines |
| `lib/api.ts` extensions | ~40 lines |
| `ChannelSurface.tsx` rewire (state model, useEffect, optimistic dispatch, error handling) | ~120 lines (~80 changed, ~40 new) |
| Date-divider helper + CSS | ~50 lines |
| `CLAUDE.md` updates (status + Resolved + Implementation Status + Open Decisions) | ~60 lines |
| `README.md` bump | ~3 lines |
| **Total** | ~730 net lines, single commit |

Larger than §4 (a3) but the components are tightly coupled — schema, helpers, IPC, React are reviewing the same end-to-end feature.

---

## 9. What this slice does not do

Named explicitly so §4 (c) and beyond have a clear inheritance:

- **No summarization.** Older-than-window turns are dropped from inference context, not summarized. §4 (c).
- **No session boundaries.** A single `conversation_session` row covers the operator's entire history at this slice. Session-end / session-start mechanics ship with summarization in §4 (c) since the summary boundaries align with session boundaries.
- **No operator-knowledge proposals.** Exile observes, but `RAPPORT-STATE-MODEL.md` §3.3's propose-confirm-reject mechanic doesn't ship here — it ships in §6 with the Dossier surface.
- **No rapport state writeback.** `RAPPORT-STATE-MODEL.md` §5.4 — rapport-event detection over each turn — is §6-shaped (calibration surface ships first, rapport state model writeback layered on after).
- **No conversation export.** A future operator-facing "export conversation" feature is out of scope. Disk is the export — `~/.coo/coo.db` is the operator's conversation, encrypted.
- **No multi-device sync.** COO is single-device per ADR-0011. The conversation lives on the device where it happened.

---

— end of §4 (b) slice plan, ready to implement after §4 (a3) lands —

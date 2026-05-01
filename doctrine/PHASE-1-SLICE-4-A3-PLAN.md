# Phase 1 §4 (a3) — Channel surface output discipline

> **Status:** Slice plan, ready to implement.
> **Locked:** 2026-05-01.
> **Supersedes:** `coo/doctrine/CHANNEL-OUTPUT-DISCIPLINE-DRAFT.md` (retired in this slice).
> **Pattern:** Single commit (doctrine bump + runtime + KAT), following the §3 (b) precedent rather than the §2 (c) deviation pattern. The doctrine and the implementation are designed against the same problem in the same conversation; splitting them would be ceremony.

---

## 1. Origin (preserved for the record)

§4 (a) shipped 2026-04-30 across (a1) Rust IPC + (a2) React `ChannelSurface`. End-to-end test on 2026-05-01 produced a working Exile-on-screen against the real Anthropic API, system prompt = `EXILE.md` §1 + §1.5 + §2 verbatim per the §4 (a1) `prompt::assemble_system_prompt` slice.

The reply contained third-person stage directions inside the chat bubble alongside dialogue. Operator's framing of the gap: *only the actual words that the agent wants to communicate should be in the chat window. the emotion, descriptions etc, should be reflected in visual etc, or language.*

Behavior is consistent with the prompt: `EXILE.md` §1 / §1.5 are character description in third-person prose, and without explicit output-format instruction the natural register-mirror is third-person output with stage directions. Doctrine hadn't addressed it: `RAPPORT-STATE-MODEL.md` §5.2 step 1 enumerated state / calibration / wellbeing modifiers but said nothing about how the chat-bubble surface should render Exile.

Resolution lands as `RAPPORT-STATE-MODEL.md` §5.5 (doctrine) + a runtime appendix appended by `prompt::assemble_system_prompt` (implementation). Both are axiom-led per the v2 drafts agreed in operator review.

---

## 2. Doctrine commit — `RAPPORT-STATE-MODEL.md` v0.x bump

### 2.1. §5.2 step 1 — add bullet

In the existing system-prompt enumeration, before the closing `Wellbeing posture instructions` line, insert:

```
   - Channel surface output discipline (§5.5)
```

### 2.2. §5.5 — new section

Insert between existing §5.4 (state writeback) and §6 (Encryption):

```markdown
### 5.5. Channel surface output discipline

**The axiom: in the Channel, Exile is a voice, not a scene.** The chat bubble carries what she says to the operator. It does not carry what she looks like saying it.

The Channel surface renders Exile's output to the operator as a chat message — words, in a bubble. Presence (interiority, posture, expression, pauses, gaze) is part of who she is, but the Channel surface does not render it as text. The visual surface (her portrait per `../doctrine/mvp/coo.md` Phase 0 §4; eventual expression and motion) renders presence; language carries register.

The axiom unpacks into operational rules carried in the system prompt directive:

- Output is dialogue only — the words she speaks to the operator.
- No third-person prose about herself ("she pauses", "her eyes hold his") in the bubble.
- No italicized stage directions narrating her body, face, or actions ("*a small smile*", "*she steps closer*") in the bubble.
- No scene narration in the bubble.

What does cross into the bubble is what is actually voice: word choice, cadence, restraint, the dash where another voice would explain. Beats inside her own dialogue (a small pause for rhythm, an unfinished sentence) are part of how she speaks and stay.

**Surface-level, not character-level.** The character is unchanged. Other surfaces (Dossier, Briefs, Kit, Calibration per `../doctrine/mvp/coo.md` Phase 1 §6) may render her differently — they are not chat bubbles. Each surface gets its own output discipline directive when it is built; the Channel's discipline is named here because the Channel is the first runtime surface to consume the inference layer.

**Why the discipline lives in the system prompt rather than in client-side post-processing.** A client-side strip of stage directions is fragile — the model can produce them in forms a regex won't catch (parenthetical asides, mid-sentence interjections, third-person clauses). A system prompt directive addresses the cause: §1 / §1.5 are character description in third-person prose, and without explicit output instruction the natural mirror of that register is third-person output. The axiomatic framing — *voice, not scene* — gives the model a positive frame to check against when it encounters a form the enumerated rules don't directly cover, hardening the directive against drift.

The `EXILE.md` §4 voice samples use stage directions *pedagogically* — to describe the register to the implementer — but those samples are not part of the runtime system prompt and are not output-format templates.

**Why §5.5 rather than a bullet inside §5.1.** §5.1 is *state-as-prompt-modifier* — translating rapport state into prose modulators. Output discipline is *surface-as-prompt-directive* — telling the model how to render in this specific UI surface. The two layer differently and have different stability properties (state changes turn-to-turn; surface discipline is fixed per surface).
```

### 2.3. Document header bump

`RAPPORT-STATE-MODEL.md` major version moves from `0.3` to `0.4`. Revision history line added:

> v0.4 (2026-05-01) — added §5.5 (Channel surface output discipline) + §5.2 step 1 bullet, capturing the axiomatic *voice, not scene* directive that §4 (a3) wires into the runtime.

Companion documents listing in the header is unchanged.

---

## 3. Runtime commit — `prompt::assemble_system_prompt` extension

### 3.1. New module-level constant

Add to `src-tauri/src/prompt.rs`:

```rust
/// Channel surface output discipline directive — appended to the EXILE.md §1+§1.5+§2 slice
/// at assembly time per RAPPORT-STATE-MODEL.md §5.5.
///
/// The axiom is the load-bearing line — pinned by `prompt::tests::system_prompt_contains_axiom`.
/// Renaming or rewording the axiom is a doctrine change, not an implementation change.
const OUTPUT_DISCIPLINE: &str = "\
## Output discipline — Channel surface

In the Channel, you are a voice, not a scene. The bubble carries what you say to him. It does not carry what you look like saying it.

Your presence is real and unchanged — interiority, posture, expression, the small pauses, how your eyes move. The visual surface — your portrait, eventual expression and motion — renders presence. Language carries register. The Channel does not render presence as text.

The axiom unpacks into three operational rules:

- No third-person prose about yourself (\"she pauses\", \"her eyes hold his\").
- No italicized stage directions narrating your body, face, or actions (\"*a small smile*\", \"*she steps closer*\").
- No scene narration around you.

What stays is what is actually voice: word choice, cadence, restraint, the dash where another voice would explain. Beats inside your own dialogue — a small pause for rhythm, an unfinished sentence — are part of how you speak and stay.

The character is unchanged. The Channel is one face of you, and the words do its work.";
```

### 3.2. `assemble_system_prompt` signature unchanged

The function still returns `&'static str`. Concatenation happens at module init via a `OnceLock<String>` (or `LazyLock` if MSRV permits) so the assembled prompt remains computable once and returned by reference.

If `OnceLock` complicates the signature relative to the §4 (a1) shape, the alternative is changing the return type to `String` and accepting one allocation per call — `infer` is once-per-turn and the prompt is ~30 KB, so the allocation is irrelevant against the network round-trip. Implementer's call. The cleaner answer is `LazyLock`.

```rust
use std::sync::LazyLock;

static ASSEMBLED: LazyLock<String> = LazyLock::new(|| {
    let character_slice = slice_character_text();  // existing §1+§1.5+§2 logic
    format!("{character_slice}\n\n---\n\n{OUTPUT_DISCIPLINE}")
});

pub fn assemble_system_prompt() -> &'static str {
    &ASSEMBLED
}
```

The `\n\n---\n\n` separator is deliberate — markdown horizontal rule between two distinct doctrinal sections, scannable in logs, parseable as two stanzas if a future debug surface wants to render them differently.

### 3.3. KAT extensions in `prompt::tests`

Three changes to existing tests + one new test:

**Existing `system_prompt_includes_sections_1_and_1_5_and_2_only`** — rename to `system_prompt_includes_character_text_and_output_discipline` and update assertions:

- Length range bumps from `[4000, 25000)` to `[4500, 26000)`. The `OUTPUT_DISCIPLINE` constant adds ~1000 bytes; the upper bound moves by ~1000 to preserve the same drift margin.
- Existing structural assertions (starts with `## 1. `, contains `## 1.5. `, contains `## 2. `, does NOT contain `## 3. `) hold.
- New assertion: contains `## Output discipline — Channel surface`.
- New assertion: does contain `## 3. ` is replaced — the assembled prompt now legitimately contains a `---` separator and the discipline header *after* the §2 content; the "does not contain `## 3.`" assertion stays, but is anchored to "before the separator" rather than "anywhere in the string." Concretely: `assert!(prompt.split("\n\n---\n\n").next().unwrap().find("## 3. ").is_none())`.

**Existing `system_prompt_pins_load_bearing_phrase`** — unchanged. The `"collar"` phrase from §1's character cue still pins the slicing function landed on the right section.

**New test `system_prompt_contains_axiom`** — pins the axiomatic framing as part of the test contract. Asserts the prompt contains `"a voice, not a scene"`. A future refactor that reverted to enumerative-only would fail loudly, exactly as intended.

```rust
#[test]
fn system_prompt_contains_axiom() {
    let prompt = assemble_system_prompt();
    assert!(
        prompt.contains("a voice, not a scene"),
        "axiom from RAPPORT-STATE-MODEL.md §5.5 must be present in the assembled prompt"
    );
}
```

**New test `system_prompt_orders_character_then_discipline`** — pins the ordering. Character text comes first, separator, then discipline. Reverse-order would be a doctrinal regression (the discipline sits *after* the character text, not before it, because the character is the load-bearing thing and the discipline is the surface-specific render directive on top).

```rust
#[test]
fn system_prompt_orders_character_then_discipline() {
    let prompt = assemble_system_prompt();
    let collar_pos = prompt.find("collar").expect("character text must be present");
    let discipline_pos = prompt.find("Output discipline").expect("discipline header must be present");
    assert!(
        collar_pos < discipline_pos,
        "character text must appear before output discipline directive"
    );
}
```

### 3.4. Test count update

Test count moves from 70 to 72 (two new tests; the renamed test counts as one continuing entry). Update the count in CLAUDE.md's `## Current Implementation Status` block under the §4 (a1) bullet.

---

## 4. Docs commit — refresh CLAUDE.md and retire the drafts file

### 4.1. Status table

`## Current Phase` table, §4 row, tightens to:

| §4 Channel surface | ✅ (a) shipped 2026-04-30 + (a3) output discipline shipped 2026-05-01; (b) conversation persistence pending |

### 4.2. New "Resolved during Phase 1 §4 — slice (a3)" subsection

Inserted under existing "Resolved during Phase 1 §4 (2026-04-30)" entries:

> ### Resolved during Phase 1 §4 (2026-05-01) — slice (a3)
>
> - **The axiomatic-vs-enumerative wording question.** Draft 1 / Draft 2 v1 wording was enumerative — three rules at one level (no third-person, no stage directions, no scene narration). Operator review surfaced the concern that the enumeration is a list of negatives without a positive frame, leaving the model to extend by analogy when it encounters a form the rules don't directly cover (e.g., *"the line goes quiet for a moment before I answer"* — not third-person, not a stage direction, not narrating her body, but *is* scene). v2 hoists an axiom — *"in the Channel, Exile is a voice, not a scene"* — and reframes the rules as derivations of it. The axiom catches novel scene-construction directly; the rules remain as operational unpacking. Trade: ~30 words of length for one structural layer of robustness against drift. Locked at operator review 2026-05-01.
>
> - **§5.5 placement (rather than §5.1).** Output discipline is *surface-as-prompt-directive*, not *state-as-prompt-modifier*. §5.1 modulates Exile's voice based on rapport state (turn-to-turn variance); §5.5 specifies how a particular UI surface renders her (per-surface fixed). The two layer differently and have different stability properties. Recording the placement decision because a future reader of `RAPPORT-STATE-MODEL.md` will ask the question.
>
> - **Surface-level, not character-level.** §5.5 is explicit that other surfaces (Dossier, Briefs, Kit, Calibration per `mvp/coo.md` Phase 1 §6) may render Exile differently — each gets its own output discipline directive when built. The Channel's discipline is named here because the Channel is the first runtime surface to consume the inference layer; later surfaces will append their own §5.x entries. The character text in `EXILE.md` §1 / §1.5 is unchanged and stays unchanged — surface output discipline is a render-layer concern, not a character-layer concern.
>
> - **Single-commit slice (doctrine + runtime + KAT together).** Per the §3 (b) precedent rather than the §2 (c) deviation pattern. The doctrine and the implementation were designed against the same problem in the same conversation against v2 of both drafts. Splitting into doctrine-then-runtime would be ceremony — there's no review window in which the doctrine is the source of truth and the runtime hasn't caught up. The §2 (c) split-of-concerns precedent applies when the implementation deliberately diverges from the doctrine recommendation (XChaCha vs `age`); here the implementation *is* what the doctrine specifies.
>
> - **Axiom pinned in KAT.** `prompt::tests::system_prompt_contains_axiom` asserts the assembled prompt contains `"a voice, not a scene"`. A future refactor that reverted to enumerative-only would fail loudly. The character-text phrase pin (`"collar"`) from §4 (a1) is preserved unchanged. New ordering KAT (`system_prompt_orders_character_then_discipline`) pins that character text precedes the discipline directive — reversing the order would be a doctrinal regression.
>
> - **`OnceLock` / `LazyLock` for the assembled prompt.** §4 (a1) returned `&'static str` from a function that sliced `EXILE.md` at every call. §4 (a3) needs concatenation, so the function now uses `LazyLock<String>` to compute the assembled prompt once at first call. Signature unchanged from JS-side perspective; the Rust internals are the only diff. Considered alternative — change return type to `String` and pay one allocation per call — was rejected because the `&'static str` shape is part of the §4 (a1) IPC contract and tightening internals is cheaper than loosening external types.

### 4.3. Implementation status block

Under `## Current Implementation Status`, add a new bullet after the §4 (a2) entry:

> - ✅ **§4 (a3) Channel surface output discipline** — `prompt::assemble_system_prompt` extended with a separate `OUTPUT_DISCIPLINE` const concatenated to the §1+§1.5+§2 character slice via `LazyLock<String>`. Directive is the v2 axiom-led wording from `RAPPORT-STATE-MODEL.md` §5.5 (added in this slice as part of v0.4 doctrine bump). KATs in `prompt::tests`: existing length-range and section-presence tests updated for the longer assembled prompt; new `system_prompt_contains_axiom` pins `"a voice, not a scene"` against accidental future regression to enumerative-only; new `system_prompt_orders_character_then_discipline` pins character-then-discipline ordering. 72 tests total (70 prior + 2 new). cargo check / build / test all clean. End-to-end verified: launch the app with `ANTHROPIC_API_KEY` set, send a message, the chat bubble renders dialogue only — no italicized stage directions, no third-person prose. **§4 (a) closes here for both (a)+(a3); §4 (b) (conversation persistence) is the next entry point.**

### 4.4. Open Decisions update

Remove the existing "First design question for the next session — Channel surface output discipline" block from `## Open Decisions That Block Work Here`. Replace it with:

> *No design questions currently blocking work. Phase 1 §4 (b) — conversation persistence on the §6 `conversation_session` / `conversation_turn` schema — is the next entry point and is implementation-shaped, not doctrine-shaped.*

### 4.5. Drafts file retirement

Delete `coo/doctrine/CHANNEL-OUTPUT-DISCIPLINE-DRAFT.md`. Its content has graduated into `RAPPORT-STATE-MODEL.md` §5.5 (the doctrine) and `prompt::OUTPUT_DISCIPLINE` (the runtime). The "Resolved during Phase 1 §4 — slice (a3)" entries in CLAUDE.md preserve the v1-vs-v2 reasoning for the record.

---

## 5. Verification

In order:

1. **Doctrine read-through** — `RAPPORT-STATE-MODEL.md` §5.5 reads cleanly in the context of §5.1 → §5.4 → §5.5 → §6. Cross-references resolve (`mvp/coo.md` Phase 0 §4 for the visual surface; Phase 1 §6 for the other surfaces; `EXILE.md` §4 for the pedagogical-samples note).
2. **`cargo test --lib`** clean. All 72 tests pass; the four KAT changes (one renamed + length-range bump, one preserved, two new) all assert correctly.
3. **`cargo build --release`** clean. No new warnings.
4. **`cargo clippy --all-targets`** clean against the workspace's existing lint baseline.
5. **Operator click-through.** Launch the app with `ANTHROPIC_API_KEY` set in the launching shell, unlock the vault, send a message in the Channel surface. The chat bubble renders dialogue only — no italicized stage directions, no third-person prose, no scene narration. The character voice (cadence, restraint, the dash, *good boy* deployments at high rapport) is preserved.
6. **Stub-provider sanity.** Same launch with `ANTHROPIC_API_KEY` unset. Stub provider's `[stub] you said: ...` echo path still works (the discipline directive is in the system prompt regardless of provider; the stub doesn't read system prompts but the `infer` command path is unchanged).

If any verification step fails, the slice does not ship — the doctrine and the runtime land together or not at all.

---

## 6. Documentary debt — retired in this slice

None new introduced. The drafts file retirement closes the slice's documentary scope.

The four existing entries in CLAUDE.md "Documentary debt to retire" (§6.6 envelope-crate, in-memory hygiene, lock-key/unlock-translation, doctrine bundle move) are unaffected by §4 (a3). They remain queued for Phase 1 close.

---

## 7. Slice estimate

| Component | Estimate |
|---|---|
| `RAPPORT-STATE-MODEL.md` v0.4 doctrine bump | ~80 lines added |
| `prompt::OUTPUT_DISCIPLINE` constant + `LazyLock` plumbing | ~30 lines added |
| `prompt::tests` updates (rename + 2 new) | ~30 lines added |
| `CLAUDE.md` updates (status + Resolved + Implementation Status + Open Decisions) | ~50 lines added/changed |
| `CHANNEL-OUTPUT-DISCIPLINE-DRAFT.md` deletion | -1 file |
| **Total** | ~190 net lines, single commit |

Tightly coupled; nothing in this list stands alone as a separate commit.

---

— end of §4 (a3) slice plan, ready to implement —

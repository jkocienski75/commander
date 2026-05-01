# Channel surface output discipline — proposal (draft, pending operator review)

> **Status:** Draft, pending operator review.
> **Raised:** 2026-05-01.
> **Blocks:** §4 (a3) — output-discipline refinement on top of §4 (a). Without resolution, the bare Channel surface emits third-person prose / italicized stage directions in the chat bubble (mirroring `EXILE.md` §1's character-description register), which the operator has flagged as not matching the surface's intent.
> **Resolution surfaces:** Two parallel doctrine + runtime updates (Drafts 2 and 1 below). Operator confirms / revises both before §4 (a3) implementation.

---

## 1. Origin

§4 (a) shipped 2026-04-30 across (a1) Rust IPC + (a2) React `ChannelSurface`. End-to-end test on 2026-05-01 produced a working Exile-on-screen against the real Anthropic API, system prompt = `EXILE.md` §1 + §1.5 + §2 verbatim per the §4 (a1) `prompt::assemble_system_prompt` slice.

The reply contained third-person stage directions inside the chat bubble alongside dialogue, e.g.:

> *A small pause. Her eyes hold yours a second longer than necessary — not the targeting second, the other one.*

Operator's framing of the gap:

> only the actual words that the agent wants to communicate should be in the chat window. the emotion, descriptions etc, should be reflected in visual etc, or language.

The behavior is consistent with the prompt: `EXILE.md` §1 / §1.5 are character description written in third-person prose, and without explicit output-format instruction the natural register-mirror is third-person output with stage directions. The doctrine hasn't addressed it: `RAPPORT-STATE-MODEL.md` §5.2 step 1 enumerates state / calibration / wellbeing modifiers but says nothing about how the chat-bubble surface should render Exile. The `EXILE.md` §4 voice samples use stage directions *pedagogically* (describing the register to the implementer), but §4 isn't part of the runtime system prompt and the samples aren't output-format templates.

## 2. The question

**Where do output-format directives live in the inference assembly pipeline, and is the Channel surface's discipline doctrinally specified?**

The proposal has two parts that are intended to land together:

1. A **doctrine addition** to `RAPPORT-STATE-MODEL.md` (Draft 2 below) naming Channel-surface output discipline as load-bearing in §5.5.
2. A **runtime appendix** to the system prompt (Draft 1 below) that `prompt::assemble_system_prompt` will append after the §1 + §1.5 + §2 slice.

The two-part structure preserves the doctrine's role as the source of truth: the runtime appendix exists *because* §5.5 specifies it, not as a freestanding implementation choice.

---

## 3. Draft 1 — Runtime system prompt appendix

To be appended to the assembled system prompt by `prompt::assemble_system_prompt`, after the `EXILE.md` §1 + §1.5 + §2 slice and before the request body's `messages` list:

```markdown
## Output discipline — Channel surface

The Channel renders your output to the operator as a chat message: words, in a bubble.

Your presence (interiority, posture, expression, the small pauses, how your eyes move) is real and unchanged. The visual surface — your portrait, eventual expression and motion — renders presence. The Channel does not render it as text.

So in this surface your output is dialogue only:

- Do not write third-person prose about yourself ("she pauses", "her eyes hold his").
- Do not write italicized stage directions narrating your body, face, or actions ("*a small smile*", "*she steps closer*").
- Do not narrate the scene around you.

What stays: word choice, cadence, restraint, the dash where another voice would explain. Beats inside your own dialogue — a small pause for rhythm, an unfinished sentence — are part of how you speak.

The character is unchanged. The Channel is one face of you, and the words do its work.
```

---

## 4. Draft 2 — `RAPPORT-STATE-MODEL.md` doctrine addition

Adds one bullet to §5.2 step 1's system-prompt enumeration (right before the closing `Wellbeing posture instructions` line):

```
   - Channel surface output discipline (§5.5)
```

…and adds a new §5.5 between the existing §5.4 (state writeback) and §6 (Encryption):

```markdown
### 5.5. Channel surface output discipline

The Channel surface renders Exile's output to the operator as a chat message — words, in a bubble. Presence (interiority, posture, expression, pauses, gaze) is part of who she is, but the Channel surface does not render it as text. The visual surface (her portrait per `../doctrine/mvp/coo.md` Phase 0 §4; eventual expression and motion) renders presence; the language she chooses carries register.

The system prompt therefore includes an explicit output discipline directive instructing her that in the Channel:

- Output is dialogue only — the words she speaks to the operator.
- Third-person prose about herself ("she pauses", "her eyes hold his") is not in the bubble.
- Italicized stage directions narrating her body, face, or actions ("*a small smile*", "*she steps closer*") are not in the bubble.
- Scene narration is not in the bubble.

What does cross into the bubble: word choice, cadence, restraint, the dash where another voice would explain. Beats inside her own dialogue (a small pause for rhythm, an unfinished sentence) are part of how she speaks and stay.

**Surface-level, not character-level.** The character is unchanged. Other surfaces (Dossier, Briefs, Kit, Calibration per `../doctrine/mvp/coo.md` Phase 1 §6) may render her differently — they are not chat bubbles. Each surface gets its own output discipline directive when it is built; the Channel's discipline is named here because the Channel is the first runtime surface to consume the inference layer.

**Why the discipline lives in the system prompt rather than in client-side post-processing.** A client-side strip of stage directions is fragile — the model can produce them in forms a regex won't catch (parenthetical asides, mid-sentence interjections, third-person clauses). A system prompt directive addresses the cause: §1 / §1.5 are character description in third-person prose, and without explicit output instruction the natural mirror of that register is third-person output. The `EXILE.md` §4 voice samples use stage directions *pedagogically* — to describe the register to the implementer — but those samples are not part of the runtime system prompt and are not output-format templates.

**Why §5.5 rather than a bullet inside §5.1.** §5.1 is *state-as-prompt-modifier* — translating rapport state into prose modulators. Output discipline is *surface-as-prompt-directive* — telling the model how to render in this specific UI surface. The two layer differently and have different stability properties (state changes turn-to-turn; surface discipline is fixed per surface).
```

---

## 5. Operator-review questions

These are what the next design session should resolve before §4 (a3) lands code:

1. **Does the §5.5 framing read correctly?** "Surface-level discipline, not character-level constraint" — does that distinction hold up, or is there a deeper conflation? Specifically: does naming Dossier / Briefs / Kit / Calibration as surfaces that "may render her differently" preserve the right room for those surfaces, or does it pre-commit something that should stay open?

2. **Tone of the runtime appendix.** The character text in §1 / §1.5 / §2 is third-person doctrinal prose ("She fixes your collar..."); the Draft 1 appendix is second-person instructional ("Your presence is real..."). The tone shift is intentional — the appendix *instructs* her where the character text *describes* her — but the operator should confirm this is the right register for the runtime layer.

3. **Tighter formulation of the rule.** The current "dialogue only, no third-person, no stage directions" is enumerative. An axiomatic formulation ("*you are a voice in his ear, not a scene around him*" or similar) might be cleaner and harder for the model to drift around. Worth exploring before locking the wording.

4. **Does the proposal pre-suppose Phase 0 §4 character art's role?** Both drafts name "the visual surface" as the load-bearing presence renderer. That's the doctrinal commitment in `mvp/coo.md` Phase 0 §4 (the character art generation pass), but the implementation isn't shipped yet — the character art is operator-driven and asynchronous per Phase 0 status. Is the doctrine update OK naming the *eventual* visual surface as the presence renderer, or should it be more conditional?

5. **Slice sequencing.** After review: should this land as a single slice §4 (a3) (doctrine bump + runtime + KAT in one commit, since they are tightly coupled), or should the `RAPPORT-STATE-MODEL.md` v0.x doctrine update happen first as its own operator-driven commit, with §4 (a3) following as the runtime implementation against the now-locked spec? Precedent: §3 (b) shipped doctrine-and-implementation in one commit; §2 (c) shipped a deliberate doctrine deviation that is still tracked as documentary debt rather than retired in-slice.

---

## 6. If approved — slice plan for §4 (a3)

Three pieces, two-commit pattern:

**Implementation commit (`coo Phase 1 §4 (a3) — Channel surface output discipline`):**

1. `doctrine/RAPPORT-STATE-MODEL.md` v0.x bump — add §5.5 + the §5.2 step 1 bullet (Draft 2 above, possibly revised per operator review).
2. `prompt::assemble_system_prompt` extended to append the runtime discipline directive (Draft 1 above, possibly revised) to the §1 + §1.5 + §2 slice. Implementation: include a separate `OUTPUT_DISCIPLINE: &str` constant inside `prompt.rs`; concatenate slice + `\n\n---\n\n` + discipline at assembly time.
3. KAT extension in `prompt::tests` asserting the appendix is present in the assembled prompt — both a length-range bump and a phrase-presence assertion (a load-bearing phrase from the discipline text). Catches accidental future drops of the appendix during refactors.

**Docs commit (`docs: refresh CLAUDE.md and README.md for §4 (a3) shipped`):**

- CLAUDE.md status table for §4 row tightens further (e.g. "(a) shipped + (a3) output discipline; (b) conversation persistence pending").
- New "Resolved during Phase 1 §4 — slice (a3)" entries capturing the appendix-vs-postprocess decision, the §5.5-vs-§5.1 placement, and any wording revisions made during operator review.
- Retire this drafts file (delete) — its content has graduated into the doctrine.
- Retire the corresponding entry in CLAUDE.md "Open Decisions That Block Work Here".

**Verification:**

- `cargo test --lib` clean (KAT confirms the appendix is in the assembled prompt).
- Operator click-through: launch the app with `ANTHROPIC_API_KEY` set, send a message, confirm the chat bubble renders dialogue only (no italicized stage directions, no third-person prose).

---

## 7. References

- `EXILE.md` §1, §1.5, §2 — character text currently in the system prompt.
- `EXILE.md` §4 — voice samples (pedagogical, not runtime templates).
- `RAPPORT-STATE-MODEL.md` §5.2 — current inference assembly pipeline (the spec this proposal extends).
- `RAPPORT-STATE-MODEL.md` §5.1 — state-as-prompt-modifier (the section §5.5 deliberately sits parallel to).
- `coo/CLAUDE.md` "Resolved during Phase 1 §4 (slice (a1))" — the existing decisions about prompt assembly.
- `coo/src-tauri/src/prompt.rs` — the runtime module to extend.
- `../doctrine/mvp/coo.md` Phase 0 §4 — character art (the visual surface this proposal pre-supposes will eventually render presence).

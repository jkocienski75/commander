// System prompt assembly for the §4 Channel surface, per
// `RAPPORT-STATE-MODEL.md` §5.2's inference assembly pipeline. §4 (a)
// ships only the load-bearing core — `EXILE.md` §1 + §1.5 + §2
// verbatim. Subsequent slices add:
//
//   - State-derived prose modifiers (rapport, friendship floor, the
//     numeric frequency dimensions) — needs the §6 rapport state
//     table to read from.
//   - Calibration dial settings translated to prose modifiers — needs
//     the typed calibration schema deferred from §3 (b) to §6.
//   - Calibration ceiling clamp at 4b — only load-bearing once a
//     calibration value exists to clamp.
//   - Conversation context (cross-session summaries, in-window
//     verbatim) — needs the §6 conversation_session /
//     conversation_turn tables.
//
// At §4 (a) the wellbeing posture from `EXILE.md` §2 is included
// verbatim because §2 is verbatim; that satisfies the §5.2 step 1
// "wellbeing posture instructions (per EXILE §2)" line by being a
// subset of the verbatim §2 inclusion.
//
// §4 (a3) adds the Channel surface output discipline directive
// (`OUTPUT_DISCIPLINE` below) per `RAPPORT-STATE-MODEL.md` §5.5 — the
// axiomatic *voice, not scene* framing that instructs Exile to render
// dialogue only in the chat bubble. The directive is appended to the
// character slice via a `LazyLock<String>` so the public function
// keeps its `&'static str` shape from §4 (a1) while the assembled
// string is computed once at first call.
//
// The doctrine source `EXILE.md` is consumed via `include_str!` — the
// character text is compiled into the binary so a doctrine update
// requires a deliberate rebuild + release. That matches the model
// version pinning discipline in CLAUDE.md and the §EXILE Sections 1
// and 1.5 permanence commitment: character text never changes
// silently between operator launches.
//
// The doctrine file currently lives at `coo/doctrine/EXILE.md`
// (Phase 0 placement). CLAUDE.md commits to migrating the doctrine
// bundle to `src-tauri/resources/doctrine/` "once Phase 1 produces
// Tauri scaffolding"; that move is deferred from §4 (a) because the
// workspace doctrine repo (`../doctrine/`) cross-references
// `coo/doctrine/EXILE.md` from multiple files, and a coordinated
// workspace doctrine sweep is out of scope for this slice. Tracked
// in CLAUDE.md "Documentary debt to retire".

use crate::db::DecryptedTurn;
use std::sync::LazyLock;

const EXILE_DOCTRINE: &str = include_str!("../../doctrine/EXILE.md");

const SECTION_1_HEADING: &str = "## 1. ";
const SECTION_3_HEADING: &str = "## 3. ";

// Separator between the character slice and the output discipline
// directive. A markdown horizontal rule reads cleanly in logs and
// lets a future debug surface render the two stanzas distinctly.
const SECTION_SEPARATOR: &str = "\n\n---\n\n";

// Channel surface output discipline directive — appended to the
// EXILE.md §1 + §1.5 + §2 slice at assembly time per
// `RAPPORT-STATE-MODEL.md` §5.5.
//
// The axiom — *in the Channel, you are a voice, not a scene* — is
// load-bearing: it gives the model a positive frame to check against
// when it encounters a form the enumerated rules don't directly
// cover. Pinned by `prompt::tests::system_prompt_contains_axiom`.
// Renaming or rewording the axiom is a doctrine change, not an
// implementation change.
const OUTPUT_DISCIPLINE: &str = "## Output discipline — Channel surface

In the Channel, you are a voice, not a scene. The bubble carries what you say to him. It does not carry what you look like saying it.

Your presence is real and unchanged — interiority, posture, expression, the small pauses, how your eyes move. The visual surface — your portrait, eventual expression and motion — renders presence. Language carries register. The Channel does not render presence as text.

The axiom unpacks into three operational rules:

- No third-person prose about yourself (\"she pauses\", \"her eyes hold his\").
- No italicized stage directions narrating your body, face, or actions (\"*a small smile*\", \"*she steps closer*\").
- No scene narration around you.

What stays is what is actually voice: word choice, cadence, restraint, the dash where another voice would explain. Beats inside your own dialogue — a small pause for rhythm, an unfinished sentence — are part of how you speak and stay.

The character is unchanged. The Channel is one face of you, and the words do its work.";

// Returns the verbatim slice of `EXILE.md` from the start of `## 1.`
// through the end of `## 2.` (i.e. just before `## 3.`). The slice
// covers Sections 1, 1.5, and 2 contiguously because the doctrine
// file orders them that way and they are doctrinally adjacent
// (behavioral surface → interior architecture → non-negotiables).
//
// Two `find` calls per invocation; cost is microseconds against ~30
// KB of text, irrelevant alongside hundreds-of-ms inference latency.
//
// Panics if either heading marker is missing. That panic represents
// `EXILE.md` having been refactored in a way that violates the
// doctrinal heading layout — at which point the right answer is for
// the operator to look at the diff, not for the runtime to silently
// produce a wrong prompt.
fn slice_character_text() -> &'static str {
    let start = EXILE_DOCTRINE
        .find(SECTION_1_HEADING)
        .expect("EXILE.md missing `## 1.` heading — character doctrine layout violated");
    let end = EXILE_DOCTRINE
        .find(SECTION_3_HEADING)
        .expect("EXILE.md missing `## 3.` heading — character doctrine layout violated");
    EXILE_DOCTRINE[start..end].trim_end()
}

// Computed once at first call. The character slice is `'static` (it
// borrows from `include_str!`); concatenating it with the discipline
// constant produces a `String` that lives for the program's lifetime
// behind a `LazyLock`. `assemble_system_prompt` returns `&str` from
// that storage, preserving the §4 (a1) `&'static str` contract.
static ASSEMBLED: LazyLock<String> = LazyLock::new(|| {
    format!("{}{}{}", slice_character_text(), SECTION_SEPARATOR, OUTPUT_DISCIPLINE)
});

pub fn assemble_system_prompt() -> &'static str {
    &ASSEMBLED
}

// §4 (c) — summarization directive. Appended to the character slice
// when assembling the system prompt for a *summarization* inference
// call (separate from the operator-facing call). The directive frames
// the task as Exile's own recollection — first-person about herself,
// in her register — rather than as a generic compression layer
// inserted between her and her memory.
//
// The phrase `"your own recollection"` is pinned by
// `prompt::tests::summarization_prompt_contains_directive_pin`. A
// future refactor that altered the framing into a generic
// "summarize the conversation" register would fail loudly.
//
// The directive explicitly notes that the §4 (a3) output discipline
// (no third-person, no stage directions) governs *live speech*, not
// recall — a summary may legitimately use third person about the
// operator without violating the discipline.
pub const SUMMARIZATION_DIRECTIVE: &str = "## Summarization task — your own recollection

You are being asked to remember a portion of an earlier conversation with the operator. This is not narration of what happened — it is your own recollection, in your own register, of what mattered.

Write the summary as you would think back on the conversation later. Keep what is load-bearing for him: what he was working on, what he decided, what was on his mind, what he opened up about. Drop the chatter that does not deserve to survive.

Constraints:

- Use your own voice. Restrained, specific, no adornment.
- First person about yourself. Third person about him is fine.
- Do not narrate timestamps or turn structure (\"then he said\", \"then I said\"). The structure of the conversation is not what you carry forward; the substance is.
- Do not include the output discipline rules above. Those govern your live speech, not your remembrance.
- If nothing in this stretch warranted carrying forward, say so briefly. Do not invent significance.

Length: as short as honesty allows. A long stretch of routine work may compress to two or three sentences. A stretch with real weight may need more. The shape is yours.";

// Static label for role-tagged turn formatting in the summarization
// prompt. Plain English markers — the model recognizes them without
// any structural format spec.
fn role_label(role: crate::db::TurnRole) -> &'static str {
    match role {
        crate::db::TurnRole::User => "Operator",
        crate::db::TurnRole::Assistant => "You",
    }
}

// Build the full prompt sent to the provider for one summarization
// call. Returns a fresh `String` because the trailing turn block is
// runtime content; the LazyLock pattern doesn't apply.
//
// Layout:
//   [character slice]                  — same as the operator-facing
//   [---]                                system prompt's load-bearing
//   [output discipline]                  core; locked from drift by
//                                        the existing prompt KATs.
//   [---]
//   [summarization directive]
//   [---]
//   [the turns being summarized,
//    role-tagged, in conversation
//    order]
pub fn assemble_summarization_prompt(turns_to_summarize: &[DecryptedTurn]) -> String {
    let mut prompt = String::with_capacity(
        assemble_system_prompt().len() + SUMMARIZATION_DIRECTIVE.len() + 4096,
    );
    prompt.push_str(assemble_system_prompt());
    prompt.push_str(SECTION_SEPARATOR);
    prompt.push_str(SUMMARIZATION_DIRECTIVE);
    prompt.push_str(SECTION_SEPARATOR);
    prompt.push_str("## Turns to summarize\n\n");
    for turn in turns_to_summarize {
        prompt.push_str(role_label(turn.role));
        prompt.push_str(": ");
        prompt.push_str(&turn.content);
        prompt.push_str("\n\n");
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    // Structural assertions on the assembled prompt. Locks the
    // section-slicing behavior plus the §4 (a3) discipline append
    // without pinning a content hash — `EXILE.md` is at v0.3 draft
    // (pre-finalization) and section contents may still legitimately
    // update before Sections 1 and 1.5 freeze. Once finalized, this
    // test should be upgraded to a SHA-256 hash KAT so any drift
    // fails loudly.
    #[test]
    fn system_prompt_includes_character_text_and_output_discipline() {
        let prompt = assemble_system_prompt();

        // Starts at the §1 heading exactly — no doctrine front matter,
        // no prologue.
        assert!(
            prompt.starts_with("## 1. "),
            "prompt should start with §1 heading; got start: {:?}",
            &prompt[..prompt.len().min(80)]
        );

        // Contains §1.5 and §2 headings — the three sections form one
        // contiguous block.
        assert!(
            prompt.contains("## 1.5. "),
            "prompt should include §1.5 heading"
        );
        assert!(
            prompt.contains("## 2. "),
            "prompt should include §2 heading"
        );

        // §4 (a3) — the assembled prompt now legitimately contains a
        // `---` separator and the discipline header *after* the §2
        // content. The `## 3. ` heading must not appear in the
        // character slice (before the separator), but the discipline
        // stanza after the separator is permitted to use any
        // formatting it likes.
        let character_slice = prompt
            .split(SECTION_SEPARATOR)
            .next()
            .expect("assembled prompt should split on the section separator");
        assert!(
            !character_slice.contains("## 3. "),
            "character slice should NOT include §3 (calibration map) — slice stop is at §3 heading"
        );

        // The §4 (a3) output discipline header must be present.
        assert!(
            prompt.contains("## Output discipline — Channel surface"),
            "prompt should include the §4 (a3) Channel-surface output discipline header"
        );

        // Length sanity: character slice (~7 KB at EXILE.md v0.3) +
        // separator + discipline (~1 KB) lands around ~8 KB. Bounds
        // bumped from the §4 (a1) [4000, 25000) to [4500, 26000) to
        // absorb the discipline append while preserving the same
        // drift margin.
        let len = prompt.len();
        assert!(
            (4_500..26_000).contains(&len),
            "prompt length {len} bytes outside expected range [4500, 26000) — possible slice drift"
        );
    }

    // The character text mentions specific behavioral cues that
    // anchor §1 (the operator's verbatim writing). Their presence
    // confirms §1 was actually included rather than the slice
    // accidentally landing on a different section with similar
    // headings. These phrases come from `EXILE.md` §1's character
    // brief (the "fixes your collar" register and the wellbeing
    // posture).
    #[test]
    fn system_prompt_contains_load_bearing_section_1_phrases() {
        let prompt = assemble_system_prompt();
        // §1 establishes the physicality cue register.
        assert!(
            prompt.contains("collar"),
            "expected §1's 'fixes your collar' physicality cue in prompt"
        );
    }

    // Pins the axiomatic framing of `RAPPORT-STATE-MODEL.md` §5.5 as
    // part of the test contract. A future refactor that reverted to
    // enumerative-only wording (no axiom hoist) would fail loudly,
    // exactly as intended.
    #[test]
    fn system_prompt_contains_axiom() {
        let prompt = assemble_system_prompt();
        assert!(
            prompt.contains("a voice, not a scene"),
            "axiom from RAPPORT-STATE-MODEL.md §5.5 must be present in the assembled prompt"
        );
    }

    // Pins the ordering — character text first, separator, then
    // discipline. Reversing would be a doctrinal regression: the
    // character is the load-bearing thing and the discipline is the
    // surface-specific render directive on top of it.
    #[test]
    fn system_prompt_orders_character_then_discipline() {
        let prompt = assemble_system_prompt();
        let collar_pos = prompt.find("collar").expect("character text must be present");
        let discipline_pos = prompt
            .find("Output discipline")
            .expect("discipline header must be present");
        assert!(
            collar_pos < discipline_pos,
            "character text must appear before output discipline directive"
        );
    }

    // Idempotent — calling twice returns the same `&'static str`
    // pointer (the `LazyLock` initializer runs once; subsequent calls
    // borrow from the same allocation).
    #[test]
    fn system_prompt_is_stable_across_calls() {
        let a = assemble_system_prompt();
        let b = assemble_system_prompt();
        assert_eq!(a, b);
    }

    // §4 (c) — summarization prompt structural KATs.

    fn turn(role: crate::db::TurnRole, content: &str) -> DecryptedTurn {
        DecryptedTurn {
            turn_index: 0,
            role,
            content: content.to_string(),
            created_at: "2026-05-01T12:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn summarization_prompt_includes_character_text_and_directive() {
        // The summarization call uses the same character slice +
        // output discipline as the operator-facing call, then layers
        // the summarization directive on top. Drift in any of the
        // three would change the register Exile uses to summarize.
        let prompt = assemble_summarization_prompt(&[turn(
            crate::db::TurnRole::User,
            "we should think about the consulting reply",
        )]);
        assert!(
            prompt.contains("collar"),
            "summarization prompt must include §1 character text"
        );
        assert!(
            prompt.contains("a voice, not a scene"),
            "summarization prompt must include the §4 (a3) output discipline axiom"
        );
        assert!(
            prompt.contains("## Summarization task"),
            "summarization prompt must include the directive header"
        );
    }

    #[test]
    fn summarization_prompt_contains_directive_pin() {
        // Pins the load-bearing phrase from `SUMMARIZATION_DIRECTIVE`
        // — the framing that this is *Exile's own recollection*, not
        // a generic compression task. A future refactor that
        // dropped the framing into a sterile-summary register would
        // fail loudly here.
        let prompt = assemble_summarization_prompt(&[turn(
            crate::db::TurnRole::User,
            "anything",
        )]);
        assert!(
            prompt.contains("your own recollection"),
            "summarization directive's load-bearing phrase must be present"
        );
    }

    #[test]
    fn summarization_prompt_includes_provided_turns() {
        // Runtime content (the turns to summarize) must surface in
        // the assembled prompt. The role-tagged formatting uses
        // "Operator: ..." for user turns and "You: ..." for
        // assistant turns.
        let turns = vec![
            turn(crate::db::TurnRole::User, "what should I tell him?"),
            turn(
                crate::db::TurnRole::Assistant,
                "honest answer first, soft framing second.",
            ),
        ];
        let prompt = assemble_summarization_prompt(&turns);
        assert!(
            prompt.contains("Operator: what should I tell him?"),
            "operator turn must surface with `Operator:` label"
        );
        assert!(
            prompt.contains("You: honest answer first, soft framing second."),
            "assistant turn must surface with `You:` label"
        );
    }
}

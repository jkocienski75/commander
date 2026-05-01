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

const EXILE_DOCTRINE: &str = include_str!("../../doctrine/EXILE.md");

const SECTION_1_HEADING: &str = "## 1. ";
const SECTION_3_HEADING: &str = "## 3. ";

// Returns the verbatim slice of `EXILE.md` from the start of `## 1.`
// through the end of `## 2.` (i.e. just before `## 3.`). The slice
// covers Sections 1, 1.5, and 2 contiguously because the doctrine
// file orders them that way and they are doctrinally adjacent
// (behavioral surface → interior architecture → non-negotiables).
//
// Returns `&'static str` — the slice borrows from the `'static`
// `include_str!` constant, so the lifetime carries through. Two
// `find` calls per invocation; cost is microseconds against ~30 KB of
// text, irrelevant alongside hundreds-of-ms inference latency.
//
// Panics if either heading marker is missing. That panic represents
// `EXILE.md` having been refactored in a way that violates the
// doctrinal heading layout — at which point the right answer is for
// the operator to look at the diff, not for the runtime to silently
// produce a wrong prompt.
pub fn assemble_system_prompt() -> &'static str {
    let start = EXILE_DOCTRINE
        .find(SECTION_1_HEADING)
        .expect("EXILE.md missing `## 1.` heading — character doctrine layout violated");
    let end = EXILE_DOCTRINE
        .find(SECTION_3_HEADING)
        .expect("EXILE.md missing `## 3.` heading — character doctrine layout violated");
    EXILE_DOCTRINE[start..end].trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Structural assertions on the assembled prompt. Locks the
    // section-slicing behavior without pinning a content hash —
    // `EXILE.md` is at v0.3 draft (pre-finalization) and section
    // contents may still legitimately update before Sections 1 and
    // 1.5 freeze. Once finalized, this test should be upgraded to a
    // SHA-256 hash KAT so any drift fails loudly.
    #[test]
    fn system_prompt_includes_sections_1_and_1_5_and_2_only() {
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

        // Stops before §3 — calibration map is not in the system
        // prompt at §4 (a). The calibration dial values translate to
        // prose modifiers in a later slice; the dial enumeration
        // itself is design-time context, not runtime instruction.
        assert!(
            !prompt.contains("## 3. "),
            "prompt should NOT include §3 (calibration map) — slice stop is at §3 heading"
        );

        // Length sanity: the three sections combined should be
        // substantial. A prompt smaller than 4 KB would mean a bad
        // slice; larger than 25 KB would mean §3+ was accidentally
        // included. Real value as of EXILE.md v0.3 is around ~7 KB.
        let len = prompt.len();
        assert!(
            (4_000..25_000).contains(&len),
            "prompt length {len} bytes outside expected range [4000, 25000) — possible slice drift"
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

    // Idempotent — calling twice returns the same `&'static str`
    // pointer (slicing a `'static` source is deterministic and the
    // function is pure).
    #[test]
    fn system_prompt_is_stable_across_calls() {
        let a = assemble_system_prompt();
        let b = assemble_system_prompt();
        assert_eq!(a, b);
    }
}

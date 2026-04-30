// Familiar preset starting calibration per EXILE.md §4 voice-sample
// header: "Calibration default for these is the 'Familiar' preset
// (Warmth: present, Directness: forthright, Mystery: held, Persistence:
// measured, Argument: forthright, Conviction: certain, Aggressive:
// protective, Flirtation: low — present, Devotion: high, Discipline:
// exacting)".
//
// The wizard writes these as the operator's starting state without a
// per-dial UI step. The dials' internal representation is deliberately
// not typed in §3 (b) (per CLAUDE.md "Resolved during Phase 1 §3" —
// EXILE.md §3 names dial endpoints but does not commit to
// enum-vs-float-vs-step quantization). The §6 Calibration surface is
// where the operator actually adjusts; this is just the seed.

export const FAMILIAR_PRESET: ReadonlyArray<readonly [string, string]> = [
  ["warmth", "present"],
  ["directness", "forthright"],
  ["mystery", "held"],
  ["persistence", "measured"],
  ["argument", "forthright"],
  ["conviction", "certain"],
  ["aggressive", "protective"],
  ["flirtation", "low — present"],
  ["devotion", "high"],
  ["discipline", "exacting"],
];

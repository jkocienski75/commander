// §4 (c) — quiet visual element rendered in place of summarized
// turns. Center-aligned, italicized, with a soft background and an
// "Earlier" prefix label that marks it as Exile's recollection
// rather than a turn. Read-only — no expand-to-show-original
// affordance at this slice (deferred to a future operator-tooling
// slice if it becomes painful to live without).

import { SummaryPayload } from "../lib/api";

export function SummaryStanza({ summary }: { summary: SummaryPayload }) {
  return (
    <div className="summary-stanza" data-kind={summary.kind}>
      <span className="summary-stanza-label">Earlier</span>
      <span className="summary-stanza-content">{summary.content}</span>
    </div>
  );
}

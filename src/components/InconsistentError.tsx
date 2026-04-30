// Renders the vault::InitState::Inconsistent error to the operator.
// Per CLAUDE.md "Resolved during Phase 1 §3" slice (a), Inconsistent
// means exactly one of (salt, sentinel) is present at <coo_dir>/. We
// surface the reason verbatim ("missing salt" / "missing sentinel")
// rather than auto-recovering — the operator should know their
// state is half-built and decide whether to repair manually or wipe
// and re-onboard.

interface Props {
  reason: string;
}

export function InconsistentError({ reason }: Props) {
  return (
    <main className="screen">
      <h1>vault state is inconsistent</h1>
      <p className="error">{reason}</p>
      <p>
        The operator-state directory at <code>~/.coo/</code> contains a
        partial vault. Resolve manually before relaunching — this is not
        an automatic recovery path.
      </p>
    </main>
  );
}

// Unlock screen for the relaunch path. Single passphrase input;
// `vault_unlock` translates wrong-passphrase / tampered-sentinel /
// inconsistent-state into distinct error strings per the §3 (a)
// boundary translation. We surface those verbatim — the operator
// should be able to distinguish "I mistyped" from "something is wrong
// with the on-disk state."

import { FormEvent, useState } from "react";
import { vaultUnlock } from "../lib/api";

interface Props {
  onUnlocked: () => void;
}

export function UnlockScreen({ onUnlocked }: Props) {
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError(null);
    setBusy(true);
    try {
      await vaultUnlock(passphrase);
      setPassphrase("");
      onUnlocked();
    } catch (err) {
      setError(typeof err === "string" ? err : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="screen">
      <h1>unlock</h1>
      <p>Enter your passphrase to unlock the vault.</p>
      <form onSubmit={submit} className="form">
        <input
          type="password"
          value={passphrase}
          onChange={(e) => setPassphrase(e.target.value)}
          placeholder="passphrase"
          autoFocus
          disabled={busy}
        />
        <button type="submit" disabled={busy || passphrase.length === 0}>
          {busy ? "unlocking…" : "unlock"}
        </button>
      </form>
      {error && <p className="error">{error}</p>}
    </main>
  );
}

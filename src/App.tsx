// Top-level routing. Calls vault_inspect on mount and after wizard
// completion; routes on (status, onboarding_completed, unlocked):
//
//   uninitialized                              → wizard (welcome)
//   initialized + !unlocked                    → unlock screen
//   initialized + unlocked + !completed        → wizard (resume at profile)
//   initialized + unlocked + completed         → channel placeholder
//   inconsistent                               → error
//
// The "unlocked + !completed" case is §3 (d)'s mid-onboarding crash
// recovery path: the operator set their passphrase + maybe wrote partial
// state, then crashed before the onboarding_completed_at marker was
// written. On relaunch they unlock the existing vault and the wizard
// resumes at the profile step. The wizard's commit() UPSERTs everything,
// so any partial pre-crash writes are idempotent.

import { useEffect, useState } from "react";
import { InspectResult, vaultInspect } from "./lib/api";
import { ChannelPlaceholder } from "./components/ChannelPlaceholder";
import { InconsistentError } from "./components/InconsistentError";
import { OnboardingWizard } from "./components/OnboardingWizard";
import { UnlockScreen } from "./components/UnlockScreen";
import "./App.css";

export default function App() {
  const [state, setState] = useState<InspectResult | null>(null);
  const [unlocked, setUnlocked] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  async function refresh() {
    try {
      setState(await vaultInspect());
    } catch (err) {
      setLoadError(typeof err === "string" ? err : String(err));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  function handleWizardComplete() {
    // Wizard ran either as first-run (vault was just unlocked by
    // vault_setup) or as resume (vault was already unlocked by
    // UnlockScreen). Either way, unlocked is now true and the
    // onboarding_completed_at marker has been written; refresh so the
    // route advances to ChannelPlaceholder.
    setUnlocked(true);
    refresh();
  }

  if (loadError) {
    return (
      <main className="screen">
        <h1>startup error</h1>
        <p className="error">{loadError}</p>
      </main>
    );
  }

  if (state === null) {
    return (
      <main className="screen">
        <p>loading…</p>
      </main>
    );
  }

  if (state.status === "inconsistent") {
    return <InconsistentError reason={state.reason} />;
  }

  if (state.status === "uninitialized") {
    return <OnboardingWizard onComplete={handleWizardComplete} />;
  }

  // state.status === "initialized"
  if (!unlocked) {
    return <UnlockScreen onUnlocked={() => setUnlocked(true)} />;
  }

  if (!state.onboarding_completed) {
    return (
      <OnboardingWizard
        onComplete={handleWizardComplete}
        startStep="profile"
      />
    );
  }

  return <ChannelPlaceholder />;
}

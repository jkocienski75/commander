// Top-level routing for §3 (c2). Calls vault_inspect on mount; routes
// based on the (status, onboarding_completed) tuple:
//
//   uninitialized               → OnboardingWizard
//   initialized + onComplete    → UnlockScreen → ChannelPlaceholder
//   initialized + !onComplete   → UnlockScreen → ChannelPlaceholder
//                                 (mid-onboarding crash recovery is a
//                                  §3 (d) startup-gating concern; for
//                                  c2 we treat it as the standard
//                                  unlock path)
//   inconsistent                → InconsistentError
//
// Real startup gating (re-inspect after unlock to verify state, route
// to wizard-resume if onboarding incomplete, etc.) lives in §3 (d).

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

  useEffect(() => {
    vaultInspect()
      .then(setState)
      .catch((err) => setLoadError(typeof err === "string" ? err : String(err)));
  }, []);

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
    return <OnboardingWizard onComplete={() => setUnlocked(true)} />;
  }

  // state.status === "initialized"
  if (unlocked) {
    return <ChannelPlaceholder />;
  }
  return <UnlockScreen onUnlocked={() => setUnlocked(true)} />;
}

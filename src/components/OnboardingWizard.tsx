// First-run onboarding wizard. Steps:
//
//   0 welcome   — preface; click to begin
//   1 passphrase — operator chooses passphrase + confirmation; on submit
//                  calls vault_setup which derives the master key
//                  (Argon2id, 250–500ms) and writes salt + sentinel;
//                  on success the vault is unlocked in managed Rust
//                  state for the rest of the session
//   2 profile    — callsign (default Cardinal-7 per EXILE.md §9.5) +
//                  display name; held in JS state until commit
//   3 review     — shows what will be written; on submit issues:
//                    write_app_config('theme', 'secret_agent')
//                    write_operator_profile(JSON.stringify(profile))
//                    write_calibration_setting(*) for each Familiar
//                                                 preset entry
//                    write_app_config('onboarding_completed_at', <iso>)
//                  the last write is the §3 (d)-readable marker that
//                  startup gating uses to skip the wizard on relaunch
//
// Per CLAUDE.md the calibration step has no per-dial UI — the Familiar
// preset is hardcoded. §6 (Calibration surface) is where the operator
// adjusts. Per EXILE.md §9.5 Cardinal-7 is the operator-confirmable
// default callsign.
//
// Errors at any step surface verbatim from Rust (the commands return
// Result<(), String>). Wizard refuses to advance past a failed write.

import { FormEvent, useState } from "react";
import {
  vaultSetup,
  writeAppConfig,
  writeCalibrationSetting,
  writeOperatorProfile,
} from "../lib/api";
import { FAMILIAR_PRESET } from "../lib/calibration";

interface Props {
  onComplete: () => void;
}

type Step = "welcome" | "passphrase" | "profile" | "review";

interface ProfileDraft {
  callsign: string;
  display_name: string;
}

export function OnboardingWizard({ onComplete }: Props) {
  const [step, setStep] = useState<Step>("welcome");
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [profile, setProfile] = useState<ProfileDraft>({
    callsign: "Cardinal-7",
    display_name: "",
  });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submitPassphrase(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    setError(null);
    if (passphrase.length < 8) {
      setError("passphrase must be at least 8 characters");
      return;
    }
    if (passphrase !== confirm) {
      setError("passphrase and confirmation do not match");
      return;
    }
    setBusy(true);
    try {
      await vaultSetup(passphrase);
      // Clear the JS-side copies as soon as the Rust side has consumed
      // them. The String allocations are not zeroized but we drop the
      // references — same in-memory hygiene posture as the Rust side.
      setPassphrase("");
      setConfirm("");
      setStep("profile");
    } catch (err) {
      setError(typeof err === "string" ? err : String(err));
    } finally {
      setBusy(false);
    }
  }

  function submitProfile(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (profile.callsign.trim().length === 0) {
      setError("callsign is required");
      return;
    }
    setStep("review");
  }

  async function commit() {
    if (busy) return;
    setError(null);
    setBusy(true);
    try {
      await writeAppConfig("theme", "secret_agent");
      await writeOperatorProfile(JSON.stringify(profile));
      for (const [dial, value] of FAMILIAR_PRESET) {
        await writeCalibrationSetting(dial, value);
      }
      await writeAppConfig("onboarding_completed_at", new Date().toISOString());
      onComplete();
    } catch (err) {
      setError(typeof err === "string" ? err : String(err));
    } finally {
      setBusy(false);
    }
  }

  if (step === "welcome") {
    return (
      <main className="screen">
        <h1>welcome to coo</h1>
        <p>
          This is the operator's personal companion application. The
          first-run setup will choose a passphrase to encrypt your local
          state, then collect your callsign and seed Exile's calibration
          to the Familiar preset.
        </p>
        <p>
          The passphrase is not stored. If you forget it, your state is
          unrecoverable — this is a real and accepted cost of the
          encryption commitment.
        </p>
        <button onClick={() => setStep("passphrase")}>begin</button>
      </main>
    );
  }

  if (step === "passphrase") {
    return (
      <main className="screen">
        <h1>set passphrase</h1>
        <form onSubmit={submitPassphrase} className="form">
          <input
            type="password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            placeholder="passphrase"
            autoFocus
            disabled={busy}
          />
          <input
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            placeholder="confirm passphrase"
            disabled={busy}
          />
          <button
            type="submit"
            disabled={busy || passphrase.length === 0 || confirm.length === 0}
          >
            {busy ? "deriving key…" : "set passphrase"}
          </button>
        </form>
        {error && <p className="error">{error}</p>}
      </main>
    );
  }

  if (step === "profile") {
    return (
      <main className="screen">
        <h1>operator profile</h1>
        <form onSubmit={submitProfile} className="form">
          <label>
            callsign
            <input
              type="text"
              value={profile.callsign}
              onChange={(e) =>
                setProfile({ ...profile, callsign: e.target.value })
              }
              autoFocus
            />
          </label>
          <label>
            display name
            <input
              type="text"
              value={profile.display_name}
              onChange={(e) =>
                setProfile({ ...profile, display_name: e.target.value })
              }
              placeholder="(optional)"
            />
          </label>
          <button type="submit">next</button>
        </form>
        {error && <p className="error">{error}</p>}
      </main>
    );
  }

  // step === "review"
  return (
    <main className="screen">
      <h1>review</h1>
      <p>About to write:</p>
      <ul className="review-list">
        <li>
          theme: <code>secret_agent</code>
        </li>
        <li>
          callsign: <code>{profile.callsign}</code>
        </li>
        {profile.display_name && (
          <li>
            display name: <code>{profile.display_name}</code>
          </li>
        )}
        <li>calibration: Familiar preset (10 dials)</li>
      </ul>
      <div className="row">
        <button onClick={() => setStep("profile")} disabled={busy}>
          back
        </button>
        <button onClick={commit} disabled={busy}>
          {busy ? "writing…" : "complete onboarding"}
        </button>
      </div>
      {error && <p className="error">{error}</p>}
    </main>
  );
}

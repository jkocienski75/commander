// Typed Tauri command wrappers for the §3 (c1) IPC surface. Tauri 2
// converts Rust snake_case parameter names to JS camelCase by default,
// so the wrappers below take camelCase args and Tauri serde-deserializes
// them back into the Rust function signatures.

import { invoke } from "@tauri-apps/api/core";

export type InspectResult =
  | { status: "uninitialized" }
  | { status: "initialized"; onboarding_completed: boolean }
  | { status: "inconsistent"; reason: string };

export const vaultInspect = (): Promise<InspectResult> =>
  invoke<InspectResult>("vault_inspect");

export const vaultSetup = (passphrase: string): Promise<void> =>
  invoke<void>("vault_setup", { passphrase });

export const vaultUnlock = (passphrase: string): Promise<void> =>
  invoke<void>("vault_unlock", { passphrase });

export const writeAppConfig = (key: string, value: string): Promise<void> =>
  invoke<void>("write_app_config", { key, value });

export const writeOperatorProfile = (plaintext: string): Promise<void> =>
  invoke<void>("write_operator_profile", { plaintext });

export const writeCalibrationSetting = (
  dialKey: string,
  plaintext: string,
): Promise<void> =>
  invoke<void>("write_calibration_setting", { dialKey, plaintext });

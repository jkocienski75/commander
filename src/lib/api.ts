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

// §4 (a) Channel surface types. `InferenceCommandError` mirrors the
// JSON-tagged enum in commands.rs; the wire shape is locked by
// commands::tests::inference_command_error_wire_shape_is_pinned on
// the Rust side. The `Message` shape used by the §4 (a1) infer
// signature is gone at §4 (b) — `infer` now takes (sessionId,
// operatorTurn) and returns `InferResponse`; the React side reads
// turns from disk via `loadConversation`.
export type InferenceCommandError =
  | { kind: "auth"; message: string }
  | { kind: "network"; message: string }
  | { kind: "rate_limited" }
  | { kind: "provider"; message: string };

// §4 (b) Channel surface conversation persistence types.
//
// `TurnRole` mirrors db::TurnRole; the Rust side serde-renames to
// snake_case but the two variant names are already snake_case-safe.
//
// `TurnPayload` mirrors commands::TurnPayload — the JSON shape
// returned by load_conversation and embedded in InferResponse's
// optimistic-replace path.
//
// `ConversationCommandError` mirrors commands::ConversationCommandError
// (locked by commands::tests::conversation_command_error_wire_shape_is_pinned).
//
// `InferResponse` mirrors commands::InferResponse (locked by
// commands::tests::infer_response_wire_shape_is_pinned).
export type TurnRole = "user" | "assistant";

export type TurnPayload = {
  turn_index: number;
  role: TurnRole;
  content: string;
  created_at: string; // ISO 8601, UTC, ms precision
};

export type LoadConversationResponse = {
  session_id: string;
  turns: TurnPayload[];
};

export type ConversationCommandError =
  | { kind: "vault_locked" }
  | { kind: "db"; message: string }
  | { kind: "crypto"; message: string };

export type InferResponse = {
  assistant_content: string;
  turn_indices: { user: number; assistant: number };
  created_at: { user: string; assistant: string };
};

export const loadConversation = (): Promise<LoadConversationResponse> =>
  invoke<LoadConversationResponse>("load_conversation");

export const infer = (
  sessionId: string,
  operatorTurn: string,
): Promise<InferResponse> =>
  invoke<InferResponse>("infer", { sessionId, operatorTurn });

// §4 (b) + §4 (c) Channel surface — conversation persistence with
// in-character summarization.
//
// Disk is the source of truth. On mount, `loadConversation` returns
// the current session_id, the turns of that session, and any
// summaries (cross-session from prior sessions + within-session for
// the current session). The component's local `messages` /
// `summaries` state is a projection of that.
//
// `dispatch(content)` is the unified send/retry helper:
//   1. If the last user turn is already optimistic with this same
//      content (turn_index = -1), this is a retry — leave the
//      messages list alone. Otherwise append an optimistic operator
//      turn so the bubble appears at typing latency.
//   2. Call `infer(session_id, content)`. The Rust side does the
//      operator-turn write, any pending summarization (within-
//      session threshold, cross-session boundary), the inference
//      round-trip, and the assistant-turn write atomically per turn.
//      On success it returns disk-authoritative turn indices,
//      timestamps, and the (possibly new) session_id.
//   3. If the returned session_id differs from the local one, the
//      inactivity-gap boundary fired inside infer and rolled the
//      session — reload conversation state from disk so the
//      scrollback shows the fresh cross-session summary stanza
//      replacing the prior session's raw turns.
//   4. Otherwise replace the optimistic operator turn with the real
//      one and append the assistant turn.
//   5. On error: keep the optimistic operator turn (don't make the
//      operator retype). Retry button calls `dispatch` again with
//      the same content; retry-detection in step 1 keeps the React
//      state from drifting.
//
// Rendering order:
//   1. Cross-session summaries in chronological generated_at order,
//      at the top of scrollback.
//   2. Current session's turns, with within-session summaries
//      replacing the covered turn ranges (turns whose turn_index
//      falls inside any within-session summary's covers_turn_range
//      are rendered as a `<SummaryStanza>` instead of a bubble).
//   3. Date dividers between turns whose calendar day differs.

import { useEffect, useRef, useState } from "react";
import {
  ConversationCommandError,
  InferenceCommandError,
  SummaryPayload,
  TurnPayload,
  infer,
  loadConversation,
} from "../lib/api";
import { SummaryStanza } from "./SummaryStanza";

const OPTIMISTIC_TURN_INDEX = -1;

export function ChannelSurface() {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<TurnPayload[]>([]);
  const [summaries, setSummaries] = useState<SummaryPayload[]>([]);
  const [input, setInput] = useState("");
  const [inflight, setInflight] = useState(false);
  const [error, setError] = useState<InferenceCommandError | null>(null);
  const [loadError, setLoadError] = useState<ConversationCommandError | null>(
    null,
  );
  const scrollRef = useRef<HTMLDivElement>(null);

  // Load on mount.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const response = await loadConversation();
        if (cancelled) return;
        setSessionId(response.session_id);
        setMessages(response.turns);
        setSummaries(response.summaries);
      } catch (err) {
        if (cancelled) return;
        setLoadError(err as ConversationCommandError);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, summaries, inflight, error]);

  async function refreshFromDisk() {
    try {
      const response = await loadConversation();
      setSessionId(response.session_id);
      setMessages(response.turns);
      setSummaries(response.summaries);
    } catch (err) {
      setLoadError(err as ConversationCommandError);
    }
  }

  async function dispatch(content: string) {
    if (!sessionId) return;
    setError(null);
    setInflight(true);

    setMessages((curr) => {
      const last = curr[curr.length - 1];
      const isRetry =
        last !== undefined &&
        last.role === "user" &&
        last.turn_index === OPTIMISTIC_TURN_INDEX &&
        last.content === content;
      if (isRetry) return curr;
      return [
        ...curr,
        {
          turn_index: OPTIMISTIC_TURN_INDEX,
          role: "user",
          content,
          created_at: new Date().toISOString(),
        },
      ];
    });

    try {
      const response = await infer(sessionId, content);

      if (response.session_id !== sessionId) {
        // §4 (c) — the inactivity-gap boundary fired inside infer.
        // The prior session was finalized + summarized + replaced
        // by a new session; a fresh load_conversation reflects the
        // disk state (new session's turns + cross-session summary
        // stanza for the prior session).
        await refreshFromDisk();
      } else {
        setMessages((curr) => {
          const replaced = curr.map((m) =>
            m.role === "user" && m.turn_index === OPTIMISTIC_TURN_INDEX
              ? {
                  ...m,
                  turn_index: response.turn_indices.user,
                  created_at: response.created_at.user,
                }
              : m,
          );
          return [
            ...replaced,
            {
              turn_index: response.turn_indices.assistant,
              role: "assistant",
              content: response.assistant_content,
              created_at: response.created_at.assistant,
            },
          ];
        });
        // §4 (c) — if the within-session threshold tripped, a new
        // summary row was written. Pull it into local state so the
        // scrollback can replace the covered turn range with a
        // SummaryStanza.
        await refreshSummaries();
      }
    } catch (err) {
      setError(err as InferenceCommandError);
    } finally {
      setInflight(false);
    }
  }

  // Pull just the summaries (and the current session's turns, since
  // their indices are needed to compute which bubbles to hide). Used
  // after a same-session infer that may have triggered the within-
  // session summarization.
  async function refreshSummaries() {
    try {
      const response = await loadConversation();
      // Don't replace messages wholesale — the optimistic-replace
      // already happened. Update summaries; if the session_id
      // changed (shouldn't at this branch, but defense in depth),
      // fall back to a full reload.
      if (response.session_id !== sessionId) {
        await refreshFromDisk();
      } else {
        setSummaries(response.summaries);
      }
    } catch (_err) {
      // Soft-fail — the conversation is still usable; the missing
      // SummaryStanza will land on the next mount.
    }
  }

  async function send() {
    const trimmed = input.trim();
    if (!trimmed || inflight || !sessionId) return;
    setInput("");
    await dispatch(trimmed);
  }

  async function retry() {
    if (inflight || !sessionId) return;
    const lastUser = [...messages].reverse().find((m) => m.role === "user");
    if (!lastUser) return;
    await dispatch(lastUser.content);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void send();
    }
  }

  if (loadError) {
    return (
      <main className="screen">
        <h1>could not load conversation</h1>
        <LoadErrorMessage error={loadError} />
      </main>
    );
  }

  if (sessionId === null) {
    return (
      <main className="screen">
        <p>loading conversation…</p>
      </main>
    );
  }

  const renderedScrollback = renderScrollback(messages, summaries);

  return (
    <main className="channel">
      <div className="channel-scrollback" ref={scrollRef}>
        {messages.length === 0 &&
          summaries.length === 0 &&
          !inflight &&
          !error && (
            <p className="channel-empty">
              The Channel is open. Speak when you're ready.
            </p>
          )}
        {renderedScrollback}
        {inflight && <div className="turn turn-inflight">…</div>}
        {error && (
          <div className="channel-error">
            <ErrorMessage error={error} />
            <button type="button" onClick={retry} disabled={inflight}>
              Retry
            </button>
          </div>
        )}
      </div>
      <div className="channel-input">
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="say something — Cmd/Ctrl+Enter to send"
          disabled={inflight}
          rows={2}
        />
        <button
          type="button"
          onClick={() => void send()}
          disabled={inflight || !input.trim()}
        >
          Send
        </button>
      </div>
    </main>
  );
}

function ErrorMessage({ error }: { error: InferenceCommandError }) {
  switch (error.kind) {
    case "auth":
      return (
        <p className="error">
          Authentication failed. Set <code>ANTHROPIC_API_KEY</code> in the
          launching shell and relaunch. ({error.message})
        </p>
      );
    case "network":
      return <p className="error">Network failure: {error.message}</p>;
    case "rate_limited":
      return (
        <p className="error">
          Rate limited by Anthropic. Wait a moment, then retry.
        </p>
      );
    case "provider":
      return <p className="error">Provider error: {error.message}</p>;
  }
}

function LoadErrorMessage({ error }: { error: ConversationCommandError }) {
  switch (error.kind) {
    case "vault_locked":
      return (
        <p className="error">
          The vault is locked. Restart the app and unlock to continue.
        </p>
      );
    case "db":
      return <p className="error">Database error: {error.message}</p>;
    case "crypto":
      return <p className="error">Decryption error: {error.message}</p>;
  }
}

// §4 (c) — render the scrollback as cross-session summaries first
// (chronological generated_at), then the current session's turns
// with within-session summaries replacing the covered ranges, with
// date dividers between turns whose calendar day differs.
function renderScrollback(
  turns: TurnPayload[],
  summaries: SummaryPayload[],
): React.ReactNode[] {
  const out: React.ReactNode[] = [];

  // Cross-session summaries at the top, oldest first.
  const crossSummaries = summaries
    .filter((s) => s.kind === "cross_session")
    .sort((a, b) => a.generated_at.localeCompare(b.generated_at));
  for (const s of crossSummaries) {
    out.push(
      <SummaryStanza
        key={`cross-${s.session_id}-${s.covers_turn_range_start}-${s.covers_turn_range_end}`}
        summary={s}
      />,
    );
  }

  // Within-session summaries — keyed by their range.
  const withinSummaries = summaries
    .filter((s) => s.kind === "within_session")
    .sort((a, b) => a.covers_turn_range_start - b.covers_turn_range_start);

  // Walk turns in order. For each turn, if it falls inside a within-
  // session summary's range, emit the summary stanza once (for the
  // first turn that hits the range) and skip remaining turns in that
  // range.
  let lastDayKey: string | null = null;
  let i = 0;
  while (i < turns.length) {
    const turn = turns[i];

    // If the turn is the first inside a within-session summary's
    // range, emit the stanza once and skip ahead past the range.
    const covering = withinSummaries.find(
      (s) =>
        turn.turn_index >= s.covers_turn_range_start &&
        turn.turn_index <= s.covers_turn_range_end,
    );
    if (covering) {
      out.push(
        <SummaryStanza
          key={`within-${covering.session_id}-${covering.covers_turn_range_start}-${covering.covers_turn_range_end}`}
          summary={covering}
        />,
      );
      while (
        i < turns.length &&
        turns[i].turn_index <= covering.covers_turn_range_end
      ) {
        i += 1;
      }
      // Reset day key after a summary stanza so the next visible
      // turn re-emits its date divider explicitly.
      lastDayKey = null;
      continue;
    }

    const date = new Date(turn.created_at);
    const dayKey = localDayKey(date);
    if (dayKey !== lastDayKey) {
      out.push(
        <div
          key={`divider-${dayKey}-${turn.turn_index}`}
          className="date-divider"
        >
          {formatDateDivider(date)}
        </div>,
      );
      lastDayKey = dayKey;
    }
    out.push(
      <div
        key={`turn-${turn.turn_index}-${turn.created_at}`}
        className={`turn turn-${turn.role}`}
      >
        {turn.content}
      </div>,
    );
    i += 1;
  }
  return out;
}

function localDayKey(date: Date): string {
  const yyyy = date.getFullYear().toString().padStart(4, "0");
  const mm = (date.getMonth() + 1).toString().padStart(2, "0");
  const dd = date.getDate().toString().padStart(2, "0");
  return `${yyyy}-${mm}-${dd}`;
}

function formatDateDivider(date: Date): string {
  const now = new Date();
  if (localDayKey(date) === localDayKey(now)) {
    return "Today";
  }
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (localDayKey(date) === localDayKey(yesterday)) {
    return "Yesterday";
  }
  return date.toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}

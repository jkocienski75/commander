// §4 (b) Channel surface — conversation persistence on top of
// §4 (a)'s text-in/text-out shape.
//
// Disk is the source of truth. On mount, `loadConversation` returns
// the operator's session_id and any turns already on disk; the
// component's local `messages` state is a projection of that.
// Refresh / restart of the app reads the same conversation back.
//
// `dispatch(content)` is the unified send/retry helper:
//   1. If the last user turn is already optimistic with this same
//      content (turn_index = -1), this is a retry — leave the
//      messages list alone. Otherwise append an optimistic operator
//      turn so the bubble appears at typing latency.
//   2. Call `infer(session_id, content)`. The Rust side does the
//      operator-turn write, the inference round-trip, and the
//      assistant-turn write atomically per turn. On success it
//      returns disk-authoritative turn indices and timestamps.
//   3. On success: replace the optimistic operator turn with the
//      real one and append the assistant turn.
//   4. On error: keep the optimistic operator turn (don't make the
//      operator retype). The Retry button calls `dispatch` again
//      with the same content; the retry-detection in step 1 keeps
//      the React state from drifting. (The slice plan accepts that
//      a real on-disk operator-turn write may have succeeded before
//      the inference call failed, in which case retry produces a
//      duplicate operator turn on disk. Local state stays clean;
//      the rare on-disk dup is acceptable at §4 (b) scope.)
//
// Date dividers: `renderTurnsWithDividers` walks the messages list
// once per render and inserts a `<DateDivider>` between turns whose
// calendar day differs (in the operator's local timezone). "Today"
// / "Yesterday" labels are computed against `now` at render time;
// re-rendering is cheap (the turn list is bounded by what the
// operator has actually said).

import { useEffect, useRef, useState } from "react";
import {
  ConversationCommandError,
  InferenceCommandError,
  TurnPayload,
  infer,
  loadConversation,
} from "../lib/api";

const OPTIMISTIC_TURN_INDEX = -1;

export function ChannelSurface() {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<TurnPayload[]>([]);
  const [input, setInput] = useState("");
  const [inflight, setInflight] = useState(false);
  const [error, setError] = useState<InferenceCommandError | null>(null);
  const [loadError, setLoadError] = useState<ConversationCommandError | null>(
    null,
  );
  const scrollRef = useRef<HTMLDivElement>(null);

  // Load the operator's conversation on mount. The Rust side creates
  // a session if none exists yet, so the response always carries a
  // session_id (the JS side never has to negotiate that).
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const response = await loadConversation();
        if (cancelled) return;
        setSessionId(response.session_id);
        setMessages(response.turns);
      } catch (err) {
        if (cancelled) return;
        setLoadError(err as ConversationCommandError);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Auto-scroll the scrollback to the bottom whenever a new message
  // lands or the inflight indicator toggles.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, inflight, error]);

  async function dispatch(content: string) {
    if (!sessionId) return;
    setError(null);
    setInflight(true);

    // If the last user turn is already optimistic with this same
    // content, treat as retry — don't append another optimistic
    // turn. This keeps the local state clean across error → retry
    // → success cycles.
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
      setMessages((curr) => {
        // Replace the optimistic operator turn (turn_index = -1)
        // with the disk-authoritative version, then append the
        // assistant turn.
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
    } catch (err) {
      setError(err as InferenceCommandError);
    } finally {
      setInflight(false);
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
    // Find the most-recent user turn — that's what failed inference.
    const lastUser = [...messages].reverse().find((m) => m.role === "user");
    if (!lastUser) return;
    await dispatch(lastUser.content);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    // Cmd/Ctrl+Enter sends. Plain Enter inserts a newline so multi-
    // paragraph turns are easy.
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

  const renderedTurns = renderTurnsWithDividers(messages);

  return (
    <main className="channel">
      <div className="channel-scrollback" ref={scrollRef}>
        {messages.length === 0 && !inflight && !error && (
          <p className="channel-empty">
            The Channel is open. Speak when you're ready.
          </p>
        )}
        {renderedTurns}
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

// Walks the turn list once and inserts a <DateDivider> between turns
// whose calendar day differs in the operator's local timezone.
// Re-runs on every render — cheap because the turn list is bounded
// by what the operator has actually said.
function renderTurnsWithDividers(turns: TurnPayload[]): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  let lastDayKey: string | null = null;
  for (const turn of turns) {
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
  }
  return out;
}

function localDayKey(date: Date): string {
  // YYYY-MM-DD in the operator's local timezone — used as the
  // grouping key for date dividers. Built without Intl to avoid
  // locale-dependent ordering surprises.
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
  // Locale-formatted absolute date for older turns.
  return date.toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}

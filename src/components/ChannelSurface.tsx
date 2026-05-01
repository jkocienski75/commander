// §4 (a2) Channel surface — the conversation surface with Exile.
// Text in / text out via the `infer` Tauri command from §4 (a1).
// System prompt assembly happens server-side; this component sees
// only the turn list.
//
// Conversation state lives in component-local useState — no
// persistence at §4 (a). Refresh / restart = empty conversation.
// Persistence + cross-session continuity is a §4 (b) concern that
// needs the §6 conversation_session / conversation_turn tables to
// land first.
//
// Error handling: the Tauri command returns Result<String,
// InferenceCommandError>; on the JS side the Promise rejects with
// the serialized InferenceCommandError shape. Each variant gets
// distinct UI: Auth points the operator at ANTHROPIC_API_KEY,
// RateLimited tells them to wait, Network and Provider show the
// underlying message. A Retry button re-runs the same `messages`
// list through `infer` — useful for transient Network / Provider /
// RateLimited errors. Auth errors require the operator to relaunch
// the app with the env var set, so retrying without that won't
// help — but the button doesn't know that, and clicking it just
// reproduces the same Auth error harmlessly.

import { useEffect, useRef, useState } from "react";
import { infer, InferenceCommandError, Message } from "../lib/api";

export function ChannelSurface() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [inflight, setInflight] = useState(false);
  const [error, setError] = useState<InferenceCommandError | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll the scrollback to the bottom whenever a new message
  // lands or the inflight indicator toggles.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, inflight, error]);

  async function dispatch(turns: Message[]) {
    setError(null);
    setInflight(true);
    try {
      const reply = await infer(turns);
      setMessages([...turns, { role: "assistant", content: reply }]);
    } catch (err) {
      setError(err as InferenceCommandError);
    } finally {
      setInflight(false);
    }
  }

  async function send() {
    const trimmed = input.trim();
    if (!trimmed || inflight) return;
    const next: Message[] = [...messages, { role: "user", content: trimmed }];
    setMessages(next);
    setInput("");
    await dispatch(next);
  }

  async function retry() {
    if (inflight || messages.length === 0) return;
    await dispatch(messages);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    // Cmd/Ctrl+Enter sends. Plain Enter inserts a newline so multi-
    // paragraph turns are easy.
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void send();
    }
  }

  return (
    <main className="channel">
      <div className="channel-scrollback" ref={scrollRef}>
        {messages.length === 0 && !inflight && !error && (
          <p className="channel-empty">
            The Channel is open. Speak when you're ready.
          </p>
        )}
        {messages.map((m, i) => (
          <div key={i} className={`turn turn-${m.role}`}>
            {m.content}
          </div>
        ))}
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

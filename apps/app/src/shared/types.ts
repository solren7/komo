// Mirror DTOs for the gateway's `/api/*` responses. Kept loose (enum fields as
// plain strings) — the GUI only displays them, so exact variant typing isn't
// worth coupling to the Rust definitions.

export interface StatusSnapshot {
  ok: boolean;
  version: string;
  channels: string[];
  home_chat: string | null;
  provider?: string;
  model?: string;
  context_window?: number | null;
  token_usage?: number | null;
  open_tasks: number;
  sessions: number;
}

export interface Task {
  id: string;
  title: string;
  note: string;
  status: string;
  board: string;
  due_at: number | null;
  created_at: number;
}

export interface Memory {
  id: string;
  kind: string;
  content: string;
  status: string;
  confidence: string;
  pinned: boolean;
}

export interface Run {
  id: string;
  session_id: string;
  input: string;
  plan: string;
  status: string;
  recoverable: boolean;
  started_at: number;
  ended_at: number | null;
  final_output: string;
  error: string;
}

export interface RunStep {
  seq: number;
  tool_name: string;
  args: string;
  result: string;
  error: string;
  ok: boolean;
  /** Measured call duration. 0 on steps recorded before the column existed, and
   *  absent entirely from a gateway older than the field — both mean "unknown",
   *  never "instant". */
  elapsed_ms?: number;
}

export interface RunDetail {
  run: Run;
  steps: RunStep[];
}

export interface SessionMessage {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  timestamp: number;
}

export interface SessionSummary {
  id: string;
  created_at: number;
  messages: number;
  user_turns: number;
  title?: string;
  /** "active" | "archive" (deleted sessions are omitted from the list). */
  status?: string;
}

export interface PendingApproval {
  summary: string;
  detail: string | null;
  risk: string;
}

export interface Interactions {
  approval: PendingApproval | null;
  question: string | null;
}

/** Turn trust mode: interactive suspends on approval/clarify, trusted
 *  auto-approves side-effecting tools (what `komo chat` does). */
export type Mode = "interactive" | "trusted";

export interface WorkspaceInfo {
  id: string;
  name: string;
  path: string;
}

/** A folder the host picked through its native directory dialog. Only a host
 *  with OS access supplies one (the desktop shell); the web build has none, and
 *  the picker entry then never renders. */
export type FolderPicker = () => Promise<{ name: string; path: string } | null>;

import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AssistantRuntimeProvider,
  AuiIf,
  SimpleTextAttachmentAdapter,
  ThreadPrimitive,
  useLocalRuntime,
  type ChatModelAdapter,
  type ThreadMessageLike,
} from "@assistant-ui/react";

import { qk } from "@/shared/api/query-keys";
import { getClient } from "@/shared/api/runtime";
import { useConnection } from "@/shared/api/use-connection";
import { pushStream } from "@/shared/lib/async";
import { useMode, useModelChoice } from "@/shared/store";
import type { PendingApproval } from "@/shared/types";
import { KomoLogo } from "@/shared/ui/komo-logo";
import { Loading } from "@/shared/ui/loading";
import { KomorebiSpinner } from "@/shared/ui/komorebi-spinner";
import { ErrorLine } from "@/shared/ui/error-line";
import { answerQuestion, decideApproval } from "./api";
import { ApprovalModal, type ApprovalDecision } from "./ApprovalModal";
import { ClarifyBar } from "./ClarifyBar";
import { Composer } from "./Composer";
import { activityToolPart, loadSessionHistory } from "./history";
import { AssistantMessage, UserMessage } from "./messages";
import { runTurn, type ToolActivity } from "./turn-orchestrator";

const textAttachments = new SimpleTextAttachmentAdapter();
textAttachments.accept += ",.txt,.md,.markdown,.csv,.json,.html,.xml,.css,.log";

/** Loads the session's history, then hands it to the runtime once. */
export function ChatView({ session, workspace }: { session: string; workspace: string }) {
  const { connected } = useConnection();
  const history = useQuery({
    queryKey: qk.sessionHistory(session),
    queryFn: () => loadSessionHistory(session),
    enabled: connected,
  });

  if (history.isPending && connected) {
    return (
      <div className="grid flex-1 place-items-center">
        <Loading>加载历史…</Loading>
      </div>
    );
  }
  if (history.isError) {
    return (
      <div className="grid flex-1 place-items-center">
        <ErrorLine error={history.error} />
      </div>
    );
  }
  return (
    <ChatThread session={session} workspace={workspace} initialMessages={history.data ?? []} />
  );
}

function ChatThread({
  session,
  workspace,
  initialMessages,
}: {
  session: string;
  workspace: string;
  initialMessages: ThreadMessageLike[];
}) {
  const mode = useMode(workspace);
  const choice = useModelChoice(session);
  const qc = useQueryClient();
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  const [question, setQuestion] = useState<string | null>(null);
  // Whether this conversation has begun — the gateway binds a session's
  // workspace when it creates the row, so once a turn has been dispatched the
  // choice is no longer the user's to make. A reloaded session with history is
  // started by definition; a fresh one flips on its first turn (not on the
  // reply, so the picker locks the moment the choice stops mattering).
  const [started, setStarted] = useState(initialMessages.length > 0);

  // One turn = one `runTurn`. The orchestrator owns the request, the live tool
  // feed, and the interaction polling; this adapter only reshapes what it
  // reports into message parts.
  //
  // It is a generator, not a plain async function, and that is the whole point:
  // each tool frame yields a fresh snapshot of the assistant message, so a
  // running call is a real tool-call part in the transcript (status `running`,
  // timing started, no result yet) rather than a separate widget alongside it.
  // When the turn lands, the same parts gain their results — nothing unmounts,
  // nothing jumps.
  //
  // `runTurn` reports by callback while a generator has to pull, so `pushStream`
  // is the join. It coalesces, so a burst of frames renders as the current state
  // instead of replaying every step.
  const adapter = useMemo<ChatModelAdapter>(
    () => ({
      async *run({ messages, abortSignal }) {
        const last = [...messages].reverse().find((m) => m.role === "user");
        const text = (last?.content ?? [])
          .map((part) => (part.type === "text" ? part.text : ""))
          .join("");

        setStarted(true);
        const feed = pushStream<ToolActivity[]>();
        const turn = runTurn(
          { session, message: text, mode, workspace, ...choice },
          { onTools: feed.push, onApproval: setApproval, onQuestion: setQuestion },
          { client: getClient(), signal: abortSignal },
        ).finally(feed.close);
        // Capture the outcome as a value. An interrupt abandons this generator
        // mid-loop, and a turn whose only rejection handler was downstream of
        // that loop would surface as an unhandled rejection instead of the
        // AbortError the runtime is waiting for.
        const settled = turn.then(
          (value) => ({ value }),
          (error: unknown) => ({ error }),
        );

        try {
          for await (const tools of feed) {
            yield { content: tools.map(activityToolPart) };
          }
        } finally {
          feed.close();
        }

        const outcome = await settled;
        if ("error" in outcome) throw outcome.error;

        // A brand-new session now exists server-side — surface it in the list.
        void qc.invalidateQueries({ queryKey: qk.sessions });
        yield {
          content: [
            ...outcome.value.tools.map(activityToolPart),
            { type: "text" as const, text: outcome.value.reply },
          ],
        };
      },
    }),
    [session, workspace, mode, choice, qc],
  );

  const runtime = useLocalRuntime(adapter, {
    initialMessages,
    adapters: { attachments: textAttachments },
  });

  const decide = (decision: ApprovalDecision) => {
    setApproval(null);
    void decideApproval(session, decision).catch(() => {});
  };

  const answer = (text: string) => {
    setQuestion(null);
    void answerQuestion(session, text).catch(() => {});
  };

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <ThreadPrimitive.Root className="flex min-h-0 min-w-0 flex-1 flex-col">
          <ThreadPrimitive.Viewport className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-6 sm:px-8">
            <AuiIf condition={(s) => s.thread.isEmpty}>
              <div className="mx-auto flex w-full max-w-xl flex-1 flex-col justify-center py-10">
                <KomoLogo className="mb-5 size-10" />
                <h1 className="text-2xl font-semibold tracking-tight">从一件要紧的事开始</h1>
                <p className="mt-2 max-w-md text-sm leading-6 text-muted-foreground">
                  让 komo 处理问题、整理上下文，或把下一步变成可执行的行动。
                </p>
              </div>
            </AuiIf>
            <ThreadPrimitive.Messages components={{ UserMessage, AssistantMessage }} />
            {/* The tool calls used to render here too, from React state fed by
                the same stream — two components drawing one thing. They now live
                in the assistant message itself, so all that is left to say is
                that the turn hasn't answered yet. */}
            <AuiIf condition={(s) => s.thread.isRunning}>
              <div
                role="status"
                className="flex items-center gap-2 px-1 text-sm text-muted-foreground"
              >
                <KomorebiSpinner />
                <span>Thinking…</span>
              </div>
            </AuiIf>
          </ThreadPrimitive.Viewport>

          {question && <ClarifyBar question={question} onAnswer={answer} />}

          <Composer session={session} workspace={workspace} started={started} />
        </ThreadPrimitive.Root>

        {approval && <ApprovalModal req={approval} onDecide={decide} />}
      </div>
    </AssistantRuntimeProvider>
  );
}

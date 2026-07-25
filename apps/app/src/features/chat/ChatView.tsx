import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AssistantRuntimeProvider,
  ThreadPrimitive,
  useLocalRuntime,
  type ChatModelAdapter,
  type ThreadMessageLike,
} from "@assistant-ui/react";

import { qk } from "@/shared/api/query-keys";
import { getClient } from "@/shared/api/runtime";
import { useConnection } from "@/shared/api/use-connection";
import { useMode, useSession } from "@/shared/store";
import type { PendingApproval } from "@/shared/types";
import { Loading } from "@/shared/ui/loading";
import { ErrorLine } from "@/shared/ui/error-line";
import { answerQuestion, decideApproval } from "./api";
import { ApprovalModal, type ApprovalDecision } from "./ApprovalModal";
import { ClarifyBar } from "./ClarifyBar";
import { Composer } from "./Composer";
import { activityToolPart, loadSessionHistory } from "./history";
import { AssistantMessage, UserMessage } from "./messages";
import { ToolActivityStrip } from "./ToolActivityStrip";
import { runTurn, type ToolActivity } from "./turn-orchestrator";

/** Loads the session's history, then hands it to the runtime once. */
export function ChatView() {
  const { connected } = useConnection();
  const session = useSession();
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
  return <ChatThread initialMessages={history.data ?? []} />;
}

function ChatThread({ initialMessages }: { initialMessages: ThreadMessageLike[] }) {
  const session = useSession();
  const mode = useMode();
  const qc = useQueryClient();
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  const [question, setQuestion] = useState<string | null>(null);
  const [tools, setTools] = useState<ToolActivity[]>([]);

  // One turn = one `runTurn`. The orchestrator owns the request, the live tool
  // feed, and the interaction polling; this component only renders what it
  // reports.
  const adapter = useMemo<ChatModelAdapter>(
    () => ({
      async run({ messages, abortSignal }) {
        const last = [...messages].reverse().find((m) => m.role === "user");
        const text = (last?.content ?? [])
          .map((part) => (part.type === "text" ? part.text : ""))
          .join("");

        const result = await runTurn(
          { session, message: text, mode },
          { onTools: setTools, onApproval: setApproval, onQuestion: setQuestion },
          { client: getClient(), signal: abortSignal },
        );

        // A brand-new session now exists server-side — surface it in the list.
        void qc.invalidateQueries({ queryKey: qk.sessions });
        setTools([]);
        return {
          content: [
            ...result.tools.map(activityToolPart),
            { type: "text" as const, text: result.reply },
          ],
        };
      },
    }),
    [session, mode, qc],
  );

  const runtime = useLocalRuntime(adapter, { initialMessages });

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
      <div className="flex min-h-0 flex-1 flex-col">
        <ThreadPrimitive.Root className="flex min-h-0 flex-1 flex-col">
          <ThreadPrimitive.Viewport className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-5">
            <ThreadPrimitive.Empty>
              <div className="flex flex-1 items-center justify-center py-10 text-muted-foreground">
                开始和 komo 对话…
              </div>
            </ThreadPrimitive.Empty>
            <ThreadPrimitive.Messages components={{ UserMessage, AssistantMessage }} />
            <ThreadPrimitive.If running>
              <ToolActivityStrip tools={tools} />
              <div className="px-1 text-sm italic text-muted-foreground">komo 正在思考…</div>
            </ThreadPrimitive.If>
          </ThreadPrimitive.Viewport>

          {question && <ClarifyBar question={question} onAnswer={answer} />}

          <Composer />
        </ThreadPrimitive.Root>

        {approval && <ApprovalModal req={approval} onDecide={decide} />}
      </div>
    </AssistantRuntimeProvider>
  );
}

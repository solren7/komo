import { MessagePrimitive, type ToolCallMessagePartProps } from "@assistant-ui/react";
import { ChevronRightIcon } from "lucide-react";

import { MarkdownText } from "@/shared/assistant-ui/markdown-text";
import { cn } from "@/shared/lib/utils";

export function UserMessage() {
  return (
    <MessagePrimitive.Root className="flex justify-end">
      <div className="max-w-[80%] rounded-2xl rounded-br-md bg-primary px-3.5 py-2 leading-relaxed break-words whitespace-pre-wrap text-primary-foreground">
        <MessagePrimitive.Parts />
      </div>
    </MessagePrimitive.Root>
  );
}

export function AssistantMessage() {
  return (
    <MessagePrimitive.Root className="flex justify-start">
      <div className="max-w-[80%] rounded-2xl rounded-bl-md border border-border bg-card px-3.5 py-2 leading-relaxed break-words text-card-foreground">
        <MessagePrimitive.Parts
          components={{ Text: MarkdownText, tools: { Override: ToolCallView } }}
        />
      </div>
    </MessagePrimitive.Root>
  );
}

/** A tool call inside an assistant message: collapsed by default, expanding to
 *  the raw arguments and the (truncated) result. */
export function ToolCallView({
  toolName,
  args,
  argsText,
  result,
  isError,
}: ToolCallMessagePartProps) {
  const skillName = toolName === "skill" && typeof args?.name === "string" ? args.name : null;
  const action = toolName === "skill" && typeof args?.action === "string" ? args.action : null;
  const title = skillName ? `Skill · ${skillName}` : toolName;
  const detail = argsText || (args ? JSON.stringify(args, null, 2) : "");
  const output =
    typeof result === "string" ? result : result == null ? "" : JSON.stringify(result, null, 2);

  return (
    // Deliberately chrome-less: a tool call is context for the reply, not the
    // reply. One quiet line that expands in place — no card, no fill, and the
    // disclosure arrow is the only affordance.
    <details className="group/tool my-0.5">
      <summary
        className={cn(
          "flex list-none items-center gap-1.5 py-0.5 text-xs transition-colors [&::-webkit-details-marker]:hidden",
          isError ? "text-destructive" : "text-muted-foreground hover:text-foreground",
        )}
      >
        <ChevronRightIcon className="size-3 shrink-0 transition-transform group-open/tool:rotate-90" />
        <span className="truncate font-mono">{title}</span>
        {action && <span className="truncate opacity-70">{action}</span>}
        {isError && <span className="shrink-0">失败</span>}
      </summary>
      {(detail || output) && (
        <div className="mt-1 mb-1.5 ml-1.5 grid gap-1.5 border-l border-border pl-3 text-xs text-muted-foreground">
          {detail && <pre className="break-all whitespace-pre-wrap">{detail}</pre>}
          {output && (
            <pre className="max-h-64 overflow-auto break-all whitespace-pre-wrap">{output}</pre>
          )}
        </div>
      )}
    </details>
  );
}

import { MessagePrimitive, type ToolCallMessagePartProps } from "@assistant-ui/react";

import { MarkdownText } from "@/shared/assistant-ui/markdown-text";

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
    <details className="my-1 overflow-hidden rounded-lg border border-border bg-muted">
      <summary className="flex items-center gap-2 px-3 py-2 text-sm select-none">
        <span className={isError ? "text-destructive" : "text-emerald-600 dark:text-emerald-400"}>
          {isError ? "✗" : "✓"}
        </span>
        <span className="font-mono font-semibold text-foreground">{title}</span>
        {action && <span className="text-xs text-muted-foreground">{action}</span>}
      </summary>
      {(detail || output) && (
        <div className="grid gap-2 border-t border-border px-3 py-2 text-xs">
          {detail && (
            <pre className="break-all whitespace-pre-wrap text-muted-foreground">{detail}</pre>
          )}
          {output && (
            <pre className="max-h-64 overflow-auto break-all whitespace-pre-wrap text-muted-foreground">
              {output}
            </pre>
          )}
        </div>
      )}
    </details>
  );
}

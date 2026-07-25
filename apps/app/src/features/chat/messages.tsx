import { MessagePrimitive, groupPartByType } from "@assistant-ui/react";

import { MarkdownText } from "@/shared/assistant-ui/markdown-text";
import { ToolCallGroup, ToolCallView } from "./ToolCalls";

export function UserMessage() {
  return (
    <MessagePrimitive.Root className="flex justify-end">
      <div className="max-w-[80%] rounded-2xl rounded-br-md bg-primary px-3.5 py-2 leading-relaxed break-words whitespace-pre-wrap text-primary-foreground">
        <MessagePrimitive.Parts />
      </div>
    </MessagePrimitive.Root>
  );
}

/** Adjacent tool calls coalesce into one `group-tool` node, which `ToolCallGroup`
 *  draws as a single collapsible line. Hoisted so the primitive can memoize its
 *  group tree on a stable reference. */
const groupToolCalls = groupPartByType({ "tool-call": ["group-tool"] });

export function AssistantMessage() {
  return (
    <MessagePrimitive.Root className="flex justify-start">
      <div className="max-w-[80%] rounded-2xl rounded-bl-md border border-border bg-card px-3.5 py-2 leading-relaxed break-words text-card-foreground">
        {/* `indicator="never"`: the thread already says "komo 正在思考…" below the
            transcript, and a reply arrives whole (the stream carries tool frames
            only), so there is nothing for a per-message indicator to show. */}
        <MessagePrimitive.GroupedParts groupBy={groupToolCalls} indicator="never">
          {({ part, children }) => {
            switch (part.type) {
              case "group-tool":
                return <ToolCallGroup indices={part.indices}>{children}</ToolCallGroup>;
              case "tool-call":
                return <ToolCallView {...part} />;
              case "text":
                return <MarkdownText />;
              default:
                return null;
            }
          }}
        </MessagePrimitive.GroupedParts>
      </div>
    </MessagePrimitive.Root>
  );
}

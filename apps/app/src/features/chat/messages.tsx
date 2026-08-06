import { MessagePrimitive, groupPartByType } from "@assistant-ui/react";

import { Markdown } from "@/shared/ui/markdown";
import { ToolCallGroup, ToolCallView } from "./ToolCalls";

export function UserMessage() {
  return (
    <MessagePrimitive.Root className="mx-auto flex w-full max-w-3xl justify-end">
      <div className="max-w-[78%] rounded-xl rounded-br-sm bg-primary px-3.5 py-2.5 leading-relaxed break-words whitespace-pre-wrap text-primary-foreground shadow-sm">
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
    <MessagePrimitive.Root className="mx-auto flex w-full max-w-3xl justify-start">
      <div className="max-w-[86%] rounded-xl rounded-bl-sm border border-border bg-card px-3.5 py-2.5 leading-relaxed break-words text-card-foreground shadow-xs">
        {/* `indicator="never"`: the thread already says "Thinking…" below the
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
                return <Markdown text={part.text} />;
              default:
                return null;
            }
          }}
        </MessagePrimitive.GroupedParts>
      </div>
    </MessagePrimitive.Root>
  );
}

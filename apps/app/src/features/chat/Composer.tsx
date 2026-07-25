import { ComposerPrimitive, unstable_useComposerInputHistory } from "@assistant-ui/react";

import { buttonVariants } from "@/shared/ui/button";
import { cn } from "@/shared/lib/utils";

/** The composer, with terminal-style input history (ArrowUp on an empty draft
 *  recalls previously sent messages). Must render inside the runtime provider —
 *  the hook reads the composer runtime. */
export function Composer() {
  const history = unstable_useComposerInputHistory();
  return (
    <ComposerPrimitive.Root className="flex items-end gap-2 border-t border-border px-4 py-3">
      <ComposerPrimitive.Input
        {...history}
        className="max-h-40 min-h-11 flex-1 resize-none rounded-xl border border-input bg-background px-3.5 py-3 font-[inherit] text-foreground outline-none transition-[color,box-shadow] focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
        placeholder="给 komo 发消息…（↑ 召回历史输入）"
      />
      <ComposerPrimitive.Send className={cn(buttonVariants({ size: "lg" }))}>
        发送
      </ComposerPrimitive.Send>
    </ComposerPrimitive.Root>
  );
}

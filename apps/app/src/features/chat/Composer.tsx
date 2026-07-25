import { AuiIf, ComposerPrimitive, unstable_useComposerInputHistory } from "@assistant-ui/react";
import { CornerDownLeftIcon, SquareIcon } from "lucide-react";

import { cn } from "@/shared/lib/utils";
import { buttonVariants } from "@/shared/ui/button";

// Both corner buttons are icon-only and chrome-less: no fill at rest, none on
// hover either (ghost's own `hover:bg-muted` is overridden in both schemes) —
// hovering only lifts the color. `buttonVariants` still supplies the sizing,
// focus ring and `disabled:opacity-50`.
const CORNER_ACTION = "hover:bg-transparent dark:hover:bg-transparent hover:text-foreground";

// Send is the *redundant* affordance — Enter already sends — so it sits back at
// muted. Stop is the only pointer route to interrupting a reply, so it stays at
// full foreground weight while it's showing.
const SEND = cn(
  buttonVariants({ variant: "ghost", size: "icon-sm" }),
  CORNER_ACTION,
  "text-muted-foreground",
);
const STOP = cn(buttonVariants({ variant: "ghost", size: "icon-sm" }), CORNER_ACTION);

/** The composer.
 *
 *  Keyboard handling is the primitive's, not ours: `ComposerPrimitive.Input`
 *  submits the surrounding form (Root *is* a `<form>`) on Enter, inserts a
 *  newline on Shift+Enter, and — the reason not to hand-roll this — ignores
 *  Enter while an IME candidate window is open, so confirming Chinese input
 *  never sends half a sentence.
 *
 *  The corner button is the pointer equivalent: send while idle, stop while a
 *  turn is running (`ComposerPrimitive.Cancel` on the thread composer cancels
 *  the *run*, which aborts the request — see turn-orchestrator.ts).
 *
 *  ArrowUp on an empty draft recalls previously sent messages (terminal-style
 *  history); the hook reads the composer runtime, so this must render inside the
 *  runtime provider. */
export function Composer() {
  const history = unstable_useComposerInputHistory();
  return (
    <ComposerPrimitive.Root className="border-t border-border px-4 py-3">
      <div className="relative">
        <ComposerPrimitive.Input
          {...history}
          className="block max-h-40 min-h-11 w-full resize-none rounded-xl border border-input bg-background py-3 pr-12 pl-3.5 font-[inherit] text-foreground outline-none transition-[color,box-shadow] focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          placeholder="给 komo 发消息…（Enter 发送 · Shift+Enter 换行 · ↑ 召回历史）"
        />
        <div className="absolute right-2 bottom-2">
          <AuiIf condition={(s) => !s.thread.isRunning}>
            <ComposerPrimitive.Send className={SEND} title="发送（Enter）">
              <CornerDownLeftIcon className="size-3.5" />
            </ComposerPrimitive.Send>
          </AuiIf>
          <AuiIf condition={(s) => s.thread.isRunning}>
            <ComposerPrimitive.Cancel className={STOP} title="中断回复">
              {/* Small but filled: a solid square is the universal stop glyph,
                  and at this size it reads without weighing the corner down. */}
              <SquareIcon className="size-3 fill-current" />
            </ComposerPrimitive.Cancel>
          </AuiIf>
        </div>
      </div>
    </ComposerPrimitive.Root>
  );
}

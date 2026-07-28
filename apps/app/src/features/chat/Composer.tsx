import { useQuery } from "@tanstack/react-query";
import {
  AttachmentPrimitive,
  AuiIf,
  ComposerPrimitive,
  unstable_useComposerInputHistory,
  useAui,
} from "@assistant-ui/react";
import {
  AtSignIcon,
  CornerDownLeftIcon,
  FileTextIcon,
  PlusIcon,
  ShieldCheckIcon,
  ShieldIcon,
  SquareIcon,
  XIcon,
} from "lucide-react";

import { fetchStatus } from "@/features/settings/api";
import {
  EffortSelect,
  ModelSelect,
  selectedContextWindow,
  useModelMenu,
} from "@/features/models/ModelPicker";
import { WorkspacePicker } from "@/features/workspaces/WorkspacePicker";
import { qk } from "@/shared/api/query-keys";
import { cn } from "@/shared/lib/utils";
import { useAppStore, useMode, useModelChoice } from "@/shared/store";
import { buttonVariants } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from "@/shared/ui/popover";

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
const TOOL =
  "inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground";

function ComposerAttachment() {
  return (
    <AttachmentPrimitive.Root className="inline-flex max-w-52 items-center gap-1.5 rounded-md border border-border bg-muted/50 px-2 py-1 text-xs">
      <FileTextIcon className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="truncate">
        <AttachmentPrimitive.Name />
      </span>
      <AttachmentPrimitive.Remove
        className="rounded-sm text-muted-foreground hover:text-foreground"
        title="移除附件"
      >
        <XIcon className="size-3" />
      </AttachmentPrimitive.Remove>
    </AttachmentPrimitive.Root>
  );
}

function formatCapacity(tokens?: number | null) {
  if (!tokens) return "—";
  return tokens >= 1_000_000
    ? `${(tokens / 1_000_000).toFixed(tokens % 1_000_000 ? 1 : 0)}M`
    : `${Math.round(tokens / 1_000)}K`;
}

function ContextProgress({ capacity, used }: { capacity?: number | null; used?: number | null }) {
  const percent =
    capacity && used != null ? Math.min(100, Math.max(0, (used / capacity) * 100)) : 0;
  return (
    <Popover>
      <PopoverTrigger
        className="relative grid size-8 place-items-center rounded-full text-[9px] font-medium text-muted-foreground"
        aria-label="上下文用量"
      >
        <svg className="absolute inset-0 size-8 -rotate-90" viewBox="0 0 36 36" aria-hidden="true">
          <circle
            cx="18"
            cy="18"
            r="15"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            className="text-muted"
          />
          {used != null && (
            <circle
              cx="18"
              cy="18"
              r="15"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.5"
              strokeLinecap="round"
              pathLength="100"
              strokeDasharray={`${percent} 100`}
              className="text-primary"
            />
          )}
        </svg>
        <span>{formatCapacity(capacity)}</span>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 gap-2">
        <PopoverTitle>上下文窗口</PopoverTitle>
        <div className="flex items-center justify-between text-xs">
          <span className="text-muted-foreground">模型容量</span>
          <span>{capacity ? capacity.toLocaleString() + " tokens" : "暂无模型容量数据"}</span>
        </div>
        <div className="flex items-center justify-between text-xs">
          <span className="text-muted-foreground">本会话用量</span>
          <span>
            {used != null
              ? `${used.toLocaleString()} tokens（${Math.round(percent)}%）`
              : "gateway 暂未提供"}
          </span>
        </div>
      </PopoverContent>
    </Popover>
  );
}

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
 *  runtime provider.
 *
 *  The workspace sits directly above the input because that is where the choice
 *  belongs: it is part of *starting* a conversation, and once the first message
 *  is sent (`started`) the gateway has bound it for good, so it degrades to a
 *  static label. Model and effort are the opposite — per session but switchable
 *  at any time — so they stay live in the control row below. */
export function Composer({
  session,
  workspace,
  started,
}: {
  session: string;
  workspace: string;
  /** The conversation already has messages, so its workspace is fixed. */
  started: boolean;
}) {
  const history = unstable_useComposerInputHistory();
  const aui = useAui();
  const mode = useMode(workspace);
  const setMode = useAppStore((s) => s.setMode);
  const setWorkspace = useAppStore((s) => s.setWorkspace);
  const choice = useModelChoice(session);
  const setModelChoice = useAppStore((s) => s.setModelChoice);
  const menu = useModelMenu();
  const status = useQuery({ queryKey: qk.status, queryFn: fetchStatus, staleTime: 30_000 });

  const insertMention = () => {
    const composer = aui.composer();
    const text = composer.getState().text;
    composer.setText(`${text}${text && !/\s$/.test(text) ? " " : ""}@`);
    requestAnimationFrame(() =>
      document.querySelector<HTMLTextAreaElement>('textarea[data-slot="komo-composer"]')?.focus(),
    );
  };

  return (
    <ComposerPrimitive.Root className="px-4 py-3">
      <div className="mb-1.5 flex min-w-0 items-center gap-2">
        <WorkspacePicker workspace={workspace} onWorkspaceChange={setWorkspace} locked={started} />
        {!started && (
          <span className="truncate text-xs text-muted-foreground">发出第一条消息后不可更改</span>
        )}
      </div>

      <div className="relative">
        <ComposerPrimitive.Input
          {...history}
          data-slot="komo-composer"
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
              <SquareIcon className="size-3" />
            </ComposerPrimitive.Cancel>
          </AuiIf>
        </div>
      </div>

      <ComposerPrimitive.Attachments components={{ Attachment: ComposerAttachment }} />

      <div className="mt-2 flex min-w-0 items-center gap-1">
        <Popover>
          <PopoverTrigger className={TOOL}>
            {mode === "trusted" ? (
              <ShieldCheckIcon className="size-3.5" />
            ) : (
              <ShieldIcon className="size-3.5" />
            )}
            {mode === "trusted" ? "信任模式" : "交互模式"}
          </PopoverTrigger>
          <PopoverContent align="start" className="w-64 gap-2 p-3">
            <PopoverTitle>信任模式</PopoverTitle>
            <p className="text-xs text-muted-foreground">
              交互模式会在有副作用的操作前请求确认；信任模式将自动批准。
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                className={cn(TOOL, mode === "interactive" && "bg-muted text-foreground")}
                onClick={() => setMode(workspace, "interactive")}
              >
                交互
              </button>
              <button
                type="button"
                className={cn(TOOL, mode === "trusted" && "bg-muted text-foreground")}
                onClick={() => setMode(workspace, "trusted")}
              >
                信任
              </button>
            </div>
          </PopoverContent>
        </Popover>

        <ComposerPrimitive.AddAttachment className={TOOL} title="添加文本附件">
          <PlusIcon className="size-4" />
        </ComposerPrimitive.AddAttachment>
        <button type="button" className={TOOL} title="插入 @" onClick={insertMention}>
          <AtSignIcon className="size-4" />
        </button>

        <span className="flex-1" />
        <ModelSelect
          menu={menu.data}
          model={choice.model}
          onModelChange={(model) => setModelChoice(session, { ...choice, model })}
        />
        <EffortSelect
          menu={menu.data}
          effort={choice.effort}
          onEffortChange={(effort) => setModelChoice(session, { ...choice, effort })}
        />
        {/* Capacity follows the session's own model, not the gateway default —
            otherwise switching to a smaller model would keep showing the big
            model's window. Usage still comes from /api/status. */}
        <ContextProgress
          capacity={selectedContextWindow(menu.data, choice.model) ?? status.data?.context_window}
          used={status.data?.token_usage}
        />
      </div>
    </ComposerPrimitive.Root>
  );
}

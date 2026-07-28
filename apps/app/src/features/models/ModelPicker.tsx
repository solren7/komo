// The composer's model + reasoning-effort controls.
//
// Both are per-session and switchable at any point in a conversation (unlike the
// workspace, which locks on the first message). The gateway is the authority on
// what may be selected: `GET /api/models` returns the menu, and the chat path
// rejects anything outside it — so an empty `efforts` here means the provider
// genuinely has no effort knob, and we say that instead of rendering a switch
// that changes nothing.

import { useQuery } from "@tanstack/react-query";

import { qk } from "@/shared/api/query-keys";
import { useConnection } from "@/shared/api/use-connection";
import { cn } from "@/shared/lib/utils";
import type { ModelMenu } from "@/shared/types";
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from "@/shared/ui/popover";
import { fetchModelMenu } from "./api";

/** Shared chrome with the composer's other inline controls. */
const CONTROL =
  "inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground";

const OPTION = "w-full rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-muted";
const OPTION_ON = "bg-muted font-medium text-foreground";

const EFFORT_LABEL: Record<string, string> = { low: "低", medium: "中", high: "高" };

export function useModelMenu() {
  const { connected } = useConnection();
  return useQuery({
    queryKey: qk.models,
    queryFn: fetchModelMenu,
    enabled: connected,
    staleTime: 5 * 60_000,
  });
}

/** The context window of the model this session runs on, or null when unknown
 *  (an id the gateway has no capacity figure for). */
export function selectedContextWindow(menu: ModelMenu | undefined, model: string): number | null {
  if (!menu) return null;
  const id = model || menu.default_model;
  return menu.models.find((option) => option.id === id)?.context_window ?? null;
}

export function ModelSelect({
  menu,
  model,
  onModelChange,
}: {
  menu: ModelMenu | undefined;
  /** Empty = whatever the gateway's default is. */
  model: string;
  onModelChange: (model: string) => void;
}) {
  const current = model || menu?.default_model || "当前模型";
  return (
    <Popover>
      <PopoverTrigger className={cn(CONTROL, "max-w-44 truncate")} title={current}>
        {current}
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64 gap-1">
        <PopoverTitle>模型</PopoverTitle>
        <p className="text-xs text-muted-foreground">按会话生效，随时可切；新会话沿用当前选择。</p>
        {menu ? (
          <div className="mt-1 flex flex-col gap-0.5">
            {menu.models.map((option) => {
              // An explicit id and the empty "use the default" value describe the
              // same model when that id *is* the default — treat them as one row
              // so the check mark can't land on nothing.
              const isDefault = option.id === menu.default_model;
              const selected = model ? model === option.id : isDefault;
              return (
                <button
                  key={option.id}
                  type="button"
                  className={cn(OPTION, selected && OPTION_ON)}
                  onClick={() => onModelChange(isDefault ? "" : option.id)}
                >
                  <span className="block truncate">{option.id}</span>
                  {isDefault && <span className="text-muted-foreground">gateway 默认</span>}
                </button>
              );
            })}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">加载模型列表…</p>
        )}
        <p className="mt-1 text-xs text-muted-foreground">
          {menu?.provider ? `provider：${menu.provider}` : ""}
          {menu && menu.models.length < 2
            ? "。在 config.toml 里配置 models = [...] 可以添加更多可选模型。"
            : ""}
        </p>
      </PopoverContent>
    </Popover>
  );
}

export function EffortSelect({
  menu,
  effort,
  onEffortChange,
}: {
  menu: ModelMenu | undefined;
  /** Empty = the provider default. */
  effort: string;
  onEffortChange: (effort: string) => void;
}) {
  const levels = menu?.efforts ?? [];
  const label = effort ? (EFFORT_LABEL[effort] ?? effort) : "自动";
  return (
    <Popover>
      <PopoverTrigger className={CONTROL} title="推理强度">
        effort · {label}
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64 gap-1">
        <PopoverTitle>推理强度</PopoverTitle>
        {levels.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            当前 provider（{menu?.provider ?? "未知"}）没有推理强度开关，使用模型默认行为。
          </p>
        ) : (
          <>
            <p className="text-xs text-muted-foreground">按会话生效，随时可切。</p>
            <div className="mt-1 flex flex-col gap-0.5">
              <button
                type="button"
                className={cn(OPTION, !effort && OPTION_ON)}
                onClick={() => onEffortChange("")}
              >
                自动（provider 默认）
              </button>
              {levels.map((level) => (
                <button
                  key={level}
                  type="button"
                  className={cn(OPTION, effort === level && OPTION_ON)}
                  onClick={() => onEffortChange(level)}
                >
                  {EFFORT_LABEL[level] ?? level}
                </button>
              ))}
            </div>
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}

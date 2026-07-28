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
import type { ModelMenu, ModelOption } from "@/shared/types";
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

/** The menu entry a session actually runs on: its explicit choice, or the
 *  gateway default when it has none. */
export function selectedOption(
  menu: ModelMenu | undefined,
  model: string,
): ModelOption | undefined {
  return menu?.models.find((option) => option.id === (model || menu.default_model));
}

/** The context window of the model this session runs on, or null when unknown
 *  (an id the gateway has no capacity figure for). */
export function selectedContextWindow(menu: ModelMenu | undefined, model: string): number | null {
  return selectedOption(menu, model)?.context_window ?? null;
}

/** Group the menu by provider, preserving the gateway's order within each group.
 *  With one provider there is nothing to label, so the caller renders it flat. */
export function byProvider(menu: ModelMenu | undefined): [string, ModelOption[]][] {
  const groups: [string, ModelOption[]][] = [];
  for (const option of menu?.models ?? []) {
    const existing = groups.find(([provider]) => provider === option.provider);
    if (existing) existing[1].push(option);
    else groups.push([option.provider, [option]]);
  }
  return groups;
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
  const groups = byProvider(menu);
  const current = selectedOption(menu, model)?.model ?? model ?? "当前模型";
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
            {groups.map(([provider, options]) => (
              <div key={provider} className="flex flex-col gap-0.5">
                {/* Only worth a heading once the menu actually spans backends. */}
                {groups.length > 1 && (
                  <span className="px-2 pt-1 text-[0.6875rem] text-muted-foreground">
                    {provider}
                  </span>
                )}
                {options.map((option) => {
                  // An explicit id and the empty "use the default" value describe
                  // the same model when that id *is* the default — treat them as
                  // one row so the check mark can't land on nothing.
                  const isDefault = option.id === menu.default_model;
                  const selected = model ? model === option.id : isDefault;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      className={cn(OPTION, selected && OPTION_ON)}
                      onClick={() => onModelChange(isDefault ? "" : option.id)}
                      title={option.id}
                    >
                      <span className="block truncate">{option.model}</span>
                      {isDefault && <span className="text-muted-foreground">gateway 默认</span>}
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">加载模型列表…</p>
        )}
        {menu && menu.models.length < 2 && (
          <p className="mt-1 text-xs text-muted-foreground">
            在 config.toml 的 models = [...] 里可以添加更多模型，跨 provider 用
            <code>deepseek:deepseek-chat</code> 这样的前缀。
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}

export function EffortSelect({
  menu,
  model,
  effort,
  onEffortChange,
}: {
  menu: ModelMenu | undefined;
  /** The session's model choice — its provider decides which levels exist. */
  model: string;
  /** Empty = the provider default. */
  effort: string;
  onEffortChange: (effort: string) => void;
}) {
  // Per selected model: switching to a provider with no effort scale must show
  // "unsupported", not the previous provider's levels.
  const selected = selectedOption(menu, model);
  const levels = selected?.efforts ?? [];
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
            {selected?.model ?? "当前模型"}（provider：
            {selected?.provider ?? menu?.provider ?? "未知"}
            ）没有推理强度开关，使用模型默认行为。
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

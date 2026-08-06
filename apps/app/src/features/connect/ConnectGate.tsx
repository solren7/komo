import { useState } from "react";

import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { KomoLogo } from "@/shared/ui/komo-logo";
import { storeGateway, storedBase } from "./gateway-storage";

/** Gate the browser build behind a key: without one there is nothing to
 *  authenticate with, so prompt for it (and an optional base for
 *  cross-origin/dev use). The desktop shell never shows this — it discovers the
 *  endpoint from `~/.komo/gateway.json`. */
export function ConnectGate({ onSaved }: { onSaved: () => void }) {
  const [base, setBase] = useState(storedBase);
  const [key, setKey] = useState("");

  const save = () => {
    const trimmed = key.trim();
    if (!trimmed) return;
    storeGateway({ base: base.trim(), key: trimmed });
    onSaved();
  };

  return (
    <div className="komo-workspace grid h-screen w-screen place-items-center overflow-x-hidden bg-background p-5 text-foreground">
      <div className="flex w-full max-w-[440px] min-w-0 flex-col gap-5 rounded-xl border border-border bg-card p-6 shadow-lg shadow-primary/5 sm:p-8">
        <div className="flex items-center gap-3">
          <KomoLogo className="size-9 shrink-0" />
          <div>
            <div className="text-lg font-semibold tracking-tight">连接 komo</div>
            <div className="mt-0.5 text-xs text-muted-foreground">本地个人 Agent 工作台</div>
          </div>
        </div>
        <p className="-mt-1 break-words text-sm leading-6 text-muted-foreground">
          输入 gateway 的访问密钥（见 <code>~/.komo/gateway.json</code> 的 <code>key</code>）。 留空
          base 表示与本页同源。
        </p>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted-foreground">Base URL（可选）</span>
          <Input
            placeholder={location.origin}
            value={base}
            onChange={(e) => setBase(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted-foreground">访问密钥</span>
          <Input
            type="password"
            autoFocus
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") save();
            }}
          />
        </label>
        <Button
          size="lg"
          className="disabled:bg-muted disabled:text-muted-foreground"
          disabled={!key.trim()}
          onClick={save}
        >
          连接
        </Button>
      </div>
    </div>
  );
}

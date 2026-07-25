import { useState } from "react";

import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
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
    <div className="grid h-screen w-screen place-items-center bg-background text-foreground">
      <div className="flex w-[min(92vw,420px)] flex-col gap-3 rounded-2xl border border-border bg-card p-6">
        <div className="flex items-center gap-2.5">
          <span className="size-7 shrink-0 rounded-lg bg-primary" />
          <div className="text-lg font-bold tracking-wide">连接 komo</div>
        </div>
        <p className="text-sm text-muted-foreground">
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
        <Button size="lg" disabled={!key.trim()} onClick={save}>
          连接
        </Button>
      </div>
    </div>
  );
}

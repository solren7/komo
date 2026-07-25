import { qk } from "@/shared/api/query-keys";
import { Badge } from "@/shared/ui/badge";
import { ErrorLine } from "@/shared/ui/error-line";
import { Loading } from "@/shared/ui/loading";
import { fetchStatus } from "../api";
import { usePanelQuery } from "../use-panel-query";

export function StatusTab() {
  const query = usePanelQuery(qk.status, fetchStatus);
  if (query.isPending) return <Loading />;
  if (query.error) return <ErrorLine error={query.error} />;
  const status = query.data!;
  const cards: [string | number, string][] = [
    [status.version, "版本"],
    [status.open_tasks, "开放任务"],
    [status.sessions, "会话数"],
    [status.home_chat ?? "—", "Home"],
  ];
  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-2.5">
        {cards.map(([value, label]) => (
          <div key={label} className="rounded-xl border border-border bg-card p-3.5 text-center">
            <div className="truncate text-[22px] font-bold text-foreground">{value}</div>
            <div className="mt-1 text-xs text-muted-foreground">{label}</div>
          </div>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-xs whitespace-nowrap text-muted-foreground">渠道：</span>
        {status.channels.length === 0 ? (
          <span className="text-sm">无</span>
        ) : (
          status.channels.map((channel) => (
            <Badge key={channel} variant="secondary">
              {channel}
            </Badge>
          ))
        )}
      </div>
    </div>
  );
}

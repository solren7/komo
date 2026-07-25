import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { qk } from "@/shared/api/query-keys";
import { fmtTs } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorLine } from "@/shared/ui/error-line";
import { Loading } from "@/shared/ui/loading";
import { fetchRunDetail, fetchRuns, RUNS_LIMIT } from "../api";
import { PANEL, ROW } from "../panel-styles";
import { usePanelQuery } from "../use-panel-query";

export function RunsTab() {
  const [open, setOpen] = useState<string | null>(null);
  const query = usePanelQuery(qk.runs(RUNS_LIMIT), () => fetchRuns(RUNS_LIMIT));
  if (query.isPending) return <Loading />;
  if (query.error) return <ErrorLine error={query.error} />;
  const runs = query.data!;
  if (runs.length === 0) return <EmptyState>还没有运行记录。</EmptyState>;
  return (
    <div className={PANEL}>
      {runs.map((run) => (
        <div key={run.id}>
          <button
            type="button"
            className={cn(ROW, "w-full text-left hover:border-primary/50")}
            onClick={() => setOpen(open === run.id ? null : run.id)}
          >
            <Badge variant="secondary">{run.status}</Badge>
            <span className="flex-1 truncate">{run.input}</span>
            {run.recoverable && (
              <Badge variant="warn" title="可恢复">
                ⟲
              </Badge>
            )}
            <span className="text-xs whitespace-nowrap text-muted-foreground">
              {fmtTs(run.started_at)}
            </span>
          </button>
          {open === run.id && <RunSteps id={run.id} />}
        </div>
      ))}
    </div>
  );
}

function RunSteps({ id }: { id: string }) {
  const query = useQuery({ queryKey: qk.run(id), queryFn: () => fetchRunDetail(id) });
  if (query.isPending)
    return (
      <div className="mt-1 ml-4 border-l-2 border-primary py-1 pl-3 text-sm text-muted-foreground">
        加载步骤…
      </div>
    );
  if (query.error)
    return (
      <div className="mt-1 ml-4 border-l-2 border-destructive py-1 pl-3">
        <ErrorLine error={query.error} />
      </div>
    );
  const { run, steps } = query.data!;
  return (
    <div className="mt-1 ml-4 flex flex-col gap-1 border-l-2 border-primary py-1 pl-3">
      {run.final_output && <div className="text-sm whitespace-pre-wrap">{run.final_output}</div>}
      {run.error && <div className="text-sm text-destructive">{run.error}</div>}
      {steps.map((step) => (
        <div className="flex items-baseline gap-2 text-sm" key={step.seq}>
          <Badge variant={step.ok ? "ok" : "destructive"}>
            {step.seq}. {step.tool_name}
          </Badge>
          <span className="truncate font-mono text-xs text-muted-foreground">{step.args}</span>
        </div>
      ))}
    </div>
  );
}

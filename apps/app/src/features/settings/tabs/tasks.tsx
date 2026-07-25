import { qk } from "@/shared/api/query-keys";
import { fmtTs } from "@/shared/lib/format";
import { Badge } from "@/shared/ui/badge";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorLine } from "@/shared/ui/error-line";
import { Loading } from "@/shared/ui/loading";
import { fetchTasks } from "../api";
import { PANEL, ROW } from "../panel-styles";
import { usePanelQuery } from "../use-panel-query";

export function TasksTab() {
  const query = usePanelQuery(qk.tasks, fetchTasks);
  if (query.isPending) return <Loading />;
  if (query.error) return <ErrorLine error={query.error} />;
  const tasks = query.data!;
  if (tasks.length === 0) return <EmptyState>没有开放任务。</EmptyState>;
  return (
    <div className={PANEL}>
      {tasks.map((task) => (
        <div className={ROW} key={task.id}>
          <Badge variant="secondary">{task.status}</Badge>
          <span className="flex-1 truncate">{task.title}</span>
          {task.board && <Badge variant="outline">#{task.board}</Badge>}
          {task.due_at != null && (
            <span className="text-xs whitespace-nowrap text-muted-foreground">
              截止 {fmtTs(task.due_at)}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

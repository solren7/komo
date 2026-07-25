import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { qk } from "@/shared/api/query-keys";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { ErrorLine } from "@/shared/ui/error-line";
import { Loading } from "@/shared/ui/loading";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/shared/ui/select";
import { actOnMemory, fetchMemories } from "../api";
import { PANEL, ROW } from "../panel-styles";
import { usePanelQuery } from "../use-panel-query";

const FILTERS: [string, string][] = [
  ["all", "全部"],
  ["candidate", "候选"],
  ["active", "活跃"],
  ["archived", "归档"],
  ["rejected", "拒绝"],
];

export function MemoriesTab() {
  const [filter, setFilter] = useState("");
  const qc = useQueryClient();
  const query = usePanelQuery(qk.memories(filter), () => fetchMemories(filter));
  const act = useMutation({
    mutationFn: ({ id, action }: { id: string; action: string }) => actOnMemory(id, action),
    onSettled: () => qc.invalidateQueries({ queryKey: ["memories"] }),
  });

  return (
    <div className={PANEL}>
      <div className="mb-1.5">
        <Select
          value={filter || "all"}
          onValueChange={(v) => setFilter(!v || v === "all" ? "" : String(v))}
        >
          <SelectTrigger size="sm" className="w-28">
            <SelectValue>
              {(value) => FILTERS.find(([v]) => v === value)?.[1] ?? "全部"}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {FILTERS.map(([value, label]) => (
              <SelectItem key={value} value={value}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {query.isPending ? (
        <Loading />
      ) : query.error ? (
        <ErrorLine error={query.error} />
      ) : query.data!.length === 0 ? (
        <EmptyState>没有记忆。</EmptyState>
      ) : (
        query.data!.map((memory) => (
          <div className={cn(ROW, "flex-col items-stretch")} key={memory.id}>
            <div className="flex items-center gap-1.5">
              <Badge variant="secondary">{memory.status}</Badge>
              <Badge variant="outline">{memory.kind}</Badge>
              {memory.pinned && <Badge variant="warn">📌</Badge>}
              <span className="text-xs whitespace-nowrap text-muted-foreground">
                {memory.confidence}
              </span>
            </div>
            <div className="my-1 break-words whitespace-pre-wrap">{memory.content}</div>
            <div className="flex gap-1.5">
              <Button size="sm" onClick={() => act.mutate({ id: memory.id, action: "promote" })}>
                promote
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => act.mutate({ id: memory.id, action: "pin" })}
              >
                pin
              </Button>
              <Button
                variant="destructive"
                size="sm"
                onClick={() => act.mutate({ id: memory.id, action: "reject" })}
              >
                reject
              </Button>
            </div>
          </div>
        ))
      )}
    </div>
  );
}

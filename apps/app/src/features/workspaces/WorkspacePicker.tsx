import { useQuery } from "@tanstack/react-query";
import { FolderIcon } from "lucide-react";

import { qk } from "@/shared/api/query-keys";
import { useConnection } from "@/shared/api/use-connection";
import { useAppStore, useWorkspace } from "@/shared/store";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/shared/ui/select";
import { fetchWorkspaces } from "./api";

export function WorkspacePicker() {
  const { connected } = useConnection();
  const workspace = useWorkspace();
  const setWorkspace = useAppStore((s) => s.setWorkspace);
  const query = useQuery({
    queryKey: qk.workspaces,
    queryFn: fetchWorkspaces,
    enabled: connected,
  });
  const items = query.data ?? [];

  return (
    <Select value={workspace} onValueChange={(value) => value && setWorkspace(value)}>
      <SelectTrigger className="h-8 w-[240px]" title="选择 workspace">
        <FolderIcon className="size-4" />
        <SelectValue placeholder={query.isPending ? "加载 workspace…" : "选择 workspace"} />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.id} value={item.id} title={item.path}>
            {item.name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

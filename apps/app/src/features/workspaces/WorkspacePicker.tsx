import { useQuery } from "@tanstack/react-query";
import { FolderIcon } from "lucide-react";

import { qk } from "@/shared/api/query-keys";
import { getFolderPicker } from "@/shared/api/runtime";
import { useConnection } from "@/shared/api/use-connection";
import { useAppStore, useWorkspace } from "@/shared/store";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/shared/ui/select";
import { fetchWorkspaces } from "./api";

/** Sentinel value for the "pick a folder" row — never a workspace id. */
const CHOOSE_FOLDER = "__choose_folder__";

/** Encode an absolute path as an opaque `folder:` workspace id.
 *
 *  The gateway resolves catalog ids by name and only decodes this form for a
 *  loopback caller (`resolve_folder_workspace` in infra/messaging/api.rs).
 *  base64url is what makes an arbitrary Unicode path safe to carry in the
 *  ASCII-only `X-Komo-Workspace` header. */
export function encodeFolder(path: string): string {
  const bytes = new TextEncoder().encode(path);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `folder:${btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "")}`;
}

export function WorkspacePicker() {
  const { connected } = useConnection();
  const workspace = useWorkspace();
  const setWorkspace = useAppStore((s) => s.setWorkspace);
  const addWorkspace = useAppStore((s) => s.addWorkspace);
  const picked = useAppStore((s) => s.pickedWorkspaces);
  const chooseFolder = getFolderPicker();
  const query = useQuery({
    queryKey: qk.workspaces,
    queryFn: fetchWorkspaces,
    enabled: connected,
  });
  // The catalog wins on a collision: a folder picked earlier that has since
  // appeared under the gateway's workspace home should read as the catalog entry.
  const items = [...(query.data ?? []), ...Object.values(picked)].filter(
    (item, index, all) => all.findIndex((candidate) => candidate.id === item.id) === index,
  );

  const choose = async () => {
    const folder = await chooseFolder?.();
    if (!folder) return;
    addWorkspace({ id: encodeFolder(folder.path), name: folder.name, path: folder.path });
  };

  return (
    <Select
      value={workspace}
      onValueChange={(value) => {
        if (value === CHOOSE_FOLDER) void choose();
        else if (value) setWorkspace(value);
      }}
    >
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
        {chooseFolder && <SelectItem value={CHOOSE_FOLDER}>选择其他文件夹…</SelectItem>}
      </SelectContent>
    </Select>
  );
}

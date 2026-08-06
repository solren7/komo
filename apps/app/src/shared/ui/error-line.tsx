import { CircleAlertIcon } from "lucide-react";

function messageFor(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  if (/\b401\b/.test(message)) {
    return "访问密钥无效或已过期。请重新连接 gateway。";
  }
  if (/fetch|network|failed to fetch/i.test(message)) {
    return "无法连接 gateway。请确认它正在运行，然后重试。";
  }
  return message;
}

/** The one inline error line. Accepts anything react-query hands back. */
export function ErrorLine({ error }: { error: unknown }) {
  return (
    <div role="alert" className="flex items-start gap-2 py-2 text-sm text-destructive">
      <CircleAlertIcon className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <span>{messageFor(error)}</span>
    </div>
  );
}

import type { PendingApproval } from "@/shared/types";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { cn } from "@/shared/lib/utils";

export type ApprovalDecision = "once" | "session" | "deny";

/** Raised while the turn is suspended server-side waiting on a human. */
export function ApprovalModal({
  req,
  onDecide,
}: {
  req: PendingApproval;
  onDecide: (decision: ApprovalDecision) => void;
}) {
  const dangerous = req.risk === "dangerous";
  return (
    <Dialog open onOpenChange={() => {}}>
      <DialogContent
        showCloseButton={false}
        className={cn("sm:max-w-120", dangerous && "ring-destructive/50")}
      >
        <DialogHeader>
          <DialogTitle>{dangerous ? "🛑 需要审批（危险操作）" : "⚠️ 需要审批"}</DialogTitle>
          <DialogDescription className="wrap-break-word text-foreground">
            {req.summary}
          </DialogDescription>
        </DialogHeader>
        {req.detail && (
          <div className="text-sm whitespace-pre-wrap text-muted-foreground">{req.detail}</div>
        )}
        <DialogFooter>
          <Button size="sm" onClick={() => onDecide("once")}>
            批准本次
          </Button>
          <Button variant="secondary" size="sm" onClick={() => onDecide("session")}>
            批准本会话
          </Button>
          <Button variant="destructive" size="sm" onClick={() => onDecide("deny")}>
            拒绝
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

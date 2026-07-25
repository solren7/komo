import { cn } from "@/shared/lib/utils";

/** A bare 24px icon affordance for dense rows (session list actions), where a
 *  full <Button> would be too heavy. */
export function IconButton({
  title,
  danger,
  onClick,
  children,
}: {
  title: string;
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={cn(
        "grid size-6 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted",
        danger ? "hover:text-destructive" : "hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

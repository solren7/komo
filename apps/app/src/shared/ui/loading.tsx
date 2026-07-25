/** The one "waiting on the gateway" line, shared by every panel. */
export function Loading({ children = "加载中…" }: { children?: React.ReactNode }) {
  return (
    <div className="flex items-center justify-center py-6 text-sm text-muted-foreground">
      {children}
    </div>
  );
}

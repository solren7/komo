/** The one "nothing here" line, shared by every panel. */
export function EmptyState({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
      {children}
    </div>
  );
}

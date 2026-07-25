/** The one inline error line. Accepts anything react-query hands back. */
export function ErrorLine({ error }: { error: unknown }) {
  return (
    <div className="py-2 text-sm text-destructive">
      {error instanceof Error ? error.message : String(error)}
    </div>
  );
}

import { cn } from "@/shared/lib/utils";

/** A quiet status mark: one mote of light moving around a faint orbit. */
export function KomorebiSpinner({ className, ...props }: React.ComponentProps<"span">) {
  return (
    <span
      aria-hidden="true"
      data-slot="komorebi-spinner"
      className={cn("komorebi-spinner size-4 shrink-0", className)}
      {...props}
    >
      <span className="komorebi-spinner-orbit" />
    </span>
  );
}

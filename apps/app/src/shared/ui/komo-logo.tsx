import { cn } from "@/shared/lib/utils";

type KomoLogoProps = React.ComponentProps<"svg"> & {
  /** Accessible name. Empty (the default) marks the logo decorative. */
  alt?: string;
};

/**
 * The komo mark, inlined from `docs/images/komo.svg` — same pixel art, with the
 * source's per-rect transforms flattened into plain coordinates.
 *
 * The viewBox crops the source's 32×32 canvas to the art itself (x 8–24, y 7–24
 * plus ~1u of padding), so the mark fills whatever box it is sized into.
 */
export function KomoLogo({ className, alt = "", ...props }: KomoLogoProps) {
  return (
    <svg
      viewBox="7 6.5 18 18"
      xmlns="http://www.w3.org/2000/svg"
      role={alt ? "img" : undefined}
      aria-label={alt || undefined}
      aria-hidden={alt ? undefined : true}
      className={cn("shrink-0", className)}
      {...props}
    >
      <rect x="20" y="15" width="1" height="1" fill="#fedfbb" />
      <rect x="20" y="15" width="1" height="1" fill="#849967" />
      <rect x="21" y="13" width="1" height="1" fill="#849967" />
      <rect x="21" y="14" width="1" height="1" fill="#849967" />
      <rect x="20" y="13" width="1" height="1" fill="#849967" />
      <rect x="15.5" y="11" width="1" height="1" fill="#a3aa57" />
      <rect x="16.5" y="8" width="1" height="2" fill="#d3d474" />
      <rect x="17.5" y="7" width="0.5" height="1" fill="#d3d474" />
      <rect x="19.5" y="7" width="1" height="3" fill="#d3d474" />
      <rect x="16.5" y="10" width="3" height="1" fill="#a3aa57" />
      <rect x="22" y="16" width="1" height="1" fill="#849967" />
      <rect x="21" y="16" width="1" height="1" fill="#849967" />
      <rect x="22" y="15" width="1" height="1" fill="#849967" />
      <rect x="21" y="15" width="1" height="1" fill="#849967" />
      <rect x="11" y="13" width="1" height="1" fill="#849967" />
      <rect x="12" y="12" width="8" height="1" fill="#849967" />
      <rect x="12" y="14" width="8" height="1" fill="#849967" />
      <rect x="12" y="13" width="8" height="1" fill="#a2b485" />
      <rect x="13.5" y="10" width="2" height="1" fill="#a3aa57" />
      <rect x="12.5" y="9" width="2" height="1" fill="#d3d474" />
      <rect x="9" y="15" width="1" height="2" fill="#849967" />
      <rect x="10" y="13" width="1" height="1" fill="#849967" />
      <rect x="10" y="14" width="1" height="1" fill="#849967" />
      <rect x="11" y="15" width="1" height="1" fill="#849967" />
      <rect x="11" y="14" width="1" height="1" fill="#849967" />
      <rect x="10" y="15" width="1" height="1" fill="#849967" />
      <rect x="10" y="16" width="1" height="1" fill="#849967" />
      <rect x="17.5" y="7" width="2" height="3" fill="#d3d474" />
      <rect x="15" y="20" width="2" height="1" fill="#637a45" />
      <rect x="18" y="18" width="1" height="2" fill="#637a45" />
      <rect x="13" y="18" width="1" height="1" fill="#637a45" />
      <rect x="13" y="18" width="1" height="2" fill="#637a45" />
      <rect x="19" y="20" width="1" height="1" fill="#fedfbb" />
      <rect x="12" y="20" width="1" height="1" fill="#fedfbb" />
      <rect x="20" y="18" width="1" height="1" fill="#f9d55f" />
      <rect x="21" y="17" width="1" height="1" fill="#f9d55f" />
      <rect x="21" y="19" width="1" height="1" fill="#f9d55f" />
      <rect x="22" y="18" width="1" height="1" fill="#f9d55f" />
      <rect x="9" y="20" width="1" height="1" fill="#849967" />
      <rect x="10" y="17" width="1" height="4" fill="#849967" />
      <rect x="9" y="17" width="1" height="3" fill="#a2b485" />
      <rect x="8" y="17" width="1" height="3" fill="#849967" />
      <rect x="23" y="17" width="1" height="3" fill="#849967" />
      <rect x="11" y="23" width="10" height="1" fill="#849967" />
      <rect x="10" y="22" width="3" height="1" fill="#849967" />
      <rect x="19" y="22" width="3" height="1" fill="#849967" />
      <rect x="20" y="21" width="2" height="1" fill="#849967" />
      <rect x="10" y="21" width="2" height="1" fill="#849967" />
      <rect x="21" y="20" width="1" height="1" fill="#a2b485" />
      <rect x="22" y="20" width="1" height="1" fill="#a2b485" />
      <rect x="22" y="19" width="1" height="1" fill="#a2b485" />
      <rect x="22" y="17" width="1" height="1" fill="#a2b485" />
      <rect x="20" y="14" width="1" height="1" fill="#849967" />
    </svg>
  );
}

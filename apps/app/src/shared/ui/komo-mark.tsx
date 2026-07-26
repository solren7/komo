import { cn } from "@/shared/lib/utils";

const komoAvatarUrl = new URL("../../../../../docs/images/komo_avatar.svg", import.meta.url).href;

type KomoMarkProps = React.ComponentProps<"span"> & {
  alt?: string;
};

/** Compact rendering of the canonical Komo avatar. */
export function KomoMark({ className, alt = "", ...props }: KomoMarkProps) {
  return (
    <span
      role={alt ? "img" : undefined}
      aria-label={alt || undefined}
      aria-hidden={alt ? undefined : true}
      className={cn("inline-block shrink-0 overflow-hidden rounded-full", className)}
      {...props}
    >
      <img
        src={komoAvatarUrl}
        alt=""
        aria-hidden="true"
        className="size-full scale-[1.8] object-cover"
      />
    </span>
  );
}

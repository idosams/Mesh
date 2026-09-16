import type { HTMLAttributes } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-full border px-2.5 py-1 text-[11px] font-semibold tracking-wide",
  {
    variants: {
      tone: {
        neutral: "border-border bg-muted text-muted-foreground",
        positive: "border-emerald-400/25 bg-emerald-400/10 text-emerald-200",
        warning: "border-amber-400/25 bg-amber-400/10 text-amber-200",
        changed: "border-sky-400/25 bg-sky-400/10 text-sky-200",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

type BadgeProps = HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>;

export function Badge({ className, tone, ...props }: BadgeProps) {
  return <span data-slot="badge" className={cn(badgeVariants({ tone }), className)} {...props} />;
}

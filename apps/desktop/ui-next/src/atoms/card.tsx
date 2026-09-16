import type { HTMLAttributes } from "react";
import { cn } from "../lib/utils";

export function Card({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return <section data-slot="card" className={cn("rounded-xl border border-border bg-card text-card-foreground shadow-sm", className)} {...props} />;
}

export function CardHeader({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div data-slot="card-header" className={cn("flex items-start justify-between gap-4 border-b border-border p-5", className)} {...props} />;
}

export function CardContent({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div data-slot="card-content" className={cn("p-5", className)} {...props} />;
}

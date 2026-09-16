import type { ReactNode } from "react";
import { Badge } from "../atoms/badge";

export type WorkspaceShellStatus = Readonly<{
  state: "checking" | "ready" | "unavailable" | "preview";
  label: string;
}>;

type WorkspaceShellProps = Readonly<{
  children: ReactNode;
  contextLabel?: string;
  status?: WorkspaceShellStatus;
}>;

const checkingStatus: WorkspaceShellStatus = Object.freeze({
  state: "checking",
  label: "Checking local service",
});

function statusTone(state: WorkspaceShellStatus["state"]): "neutral" | "positive" | "warning" | "changed" {
  if (state === "ready") return "positive";
  if (state === "unavailable") return "warning";
  if (state === "checking") return "changed";
  return "neutral";
}

export function WorkspaceShell({
  children,
  contextLabel = "Private workspace",
  status = checkingStatus,
}: WorkspaceShellProps) {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="sticky top-0 z-10 border-b border-border bg-background/90 backdrop-blur-xl">
        <div className="mx-auto flex min-h-16 max-w-7xl flex-wrap items-center justify-between gap-3 px-4 py-3 sm:px-5 lg:px-8">
          <div className="flex min-w-0 items-center gap-3">
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-xl bg-primary font-bold text-primary-foreground">M</span>
            <div className="min-w-0">
              <strong className="block text-sm">Mesh</strong>
              <span className="block truncate text-xs text-muted-foreground">{contextLabel}</span>
            </div>
          </div>
          <Badge
            className="shrink-0"
            tone={statusTone(status.state)}
            role="status"
            aria-live="polite"
            aria-atomic="true"
            data-state={status.state}
          >
            {status.label}
          </Badge>
        </div>
      </header>
      <main className="mx-auto max-w-7xl px-5 py-8 lg:px-8">{children}</main>
    </div>
  );
}

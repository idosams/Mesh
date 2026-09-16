import { Badge } from "../atoms/badge";
import type { WorkspaceChromeModel } from "../models/workspace-chrome";

export function WorkspaceHeader({ model }: { model: WorkspaceChromeModel }) {
  const tone = model.serviceState === "ready" ? "positive" : model.serviceState === "attention" ? "warning" : "changed";
  return (
    <header className="flex min-h-16 flex-wrap items-center justify-between gap-3 py-3">
      <div className="flex min-w-0 items-center gap-3">
        <span aria-hidden="true" className="grid h-9 w-9 shrink-0 place-items-center rounded-xl bg-primary font-bold text-primary-foreground">M</span>
        <div className="min-w-0">
          <strong className="block text-sm">Mesh</strong>
          <span className="block truncate text-xs text-muted-foreground">Local workspace</span>
        </div>
      </div>
      <Badge tone={tone} role="status" aria-live="polite" aria-atomic="true" data-state={model.serviceState}>
        {model.serviceLabel}
      </Badge>
    </header>
  );
}

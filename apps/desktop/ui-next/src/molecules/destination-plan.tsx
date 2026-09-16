import { Badge } from "../atoms/badge";
import type { WorkspaceDestinationPlan } from "../models/workspace-destination";

const presentation = {
  ready: { label: "Ready to confirm", tone: "changed" },
  information: { label: "Verified plan", tone: "neutral" },
  blocked: { label: "Kept safe", tone: "warning" },
  complete: { label: "Update complete", tone: "positive" },
} as const;

export function DestinationPlan({ plan }: Readonly<{ plan: WorkspaceDestinationPlan }>) {
  const state = presentation[plan.state];
  return (
    <section className="grid gap-3 rounded-xl border border-border bg-card/60 p-4" aria-labelledby="destination-plan-title">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="destination-plan-title" className="text-sm font-semibold">Exact destination plan</h3>
        <Badge tone={state.tone}>{state.label}</Badge>
      </div>
      <pre data-mesh-proof="destination-plan" role="status" aria-live="polite" aria-atomic="true" className="max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-muted/60 p-4 font-mono text-xs leading-5 text-foreground">{plan.text}</pre>
    </section>
  );
}

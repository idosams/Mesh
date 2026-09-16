import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import type {
  WorkspaceCurrentActionId,
  WorkspaceCurrentIntent,
  WorkspaceCurrentModel,
} from "../models/workspace-current";

type IntentHandler = (intent: WorkspaceCurrentIntent) => void;

export function WorkspaceCurrent({ model, onIntent }: {
  model: WorkspaceCurrentModel;
  onIntent: IntentHandler;
}) {
  const byId = new Map(model.actions.map((action) => [action.id, action]));
  const proofByAction: Partial<Record<WorkspaceCurrentActionId, string>> = {
    "start-codex": "current-start-codex",
    "finish-agent": "current-finish-agent",
  };
  const action = (id: WorkspaceCurrentActionId, variant: "primary" | "secondary" | "quiet" | "danger" = "secondary") => {
    const item = byId.get(id);
    return item ? (
      <Button
        key={id}
        data-mesh-proof={proofByAction[id]}
        aria-label={id === "refresh" ? "Refresh workspace state" : undefined}
        variant={variant}
        disabled={!item.enabled}
        onClick={() => onIntent({ type: "activate", action: id })}
      >
        {item.label}
      </Button>
    ) : null;
  };

  return (
    <div data-mesh-proof="current-mounted" aria-label="Current workspace details" className="grid gap-5 p-5 lg:p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Workspace details</p>
            <Badge tone={model.state === "Needs attention" ? "warning" : model.state === "Working" ? "neutral" : "positive"}>
              {model.state}
            </Badge>
            <span data-mesh-proof={model.agentAssigned ? "current-agent-assigned" : "current-agent-available"}>
              {model.agentAssigned ? <Badge tone="warning">Agent folder assigned</Badge> : <Badge tone="neutral">Agent folder available</Badge>}
            </span>
          </div>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">Current workspace</h2>
          <p className="mt-1 text-sm text-muted-foreground">{model.recordSummary}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          {action("return-workspace", "quiet")}
          {action("refresh", "quiet")}
        </div>
      </header>

      <div className="grid gap-4 xl:grid-cols-2">
        <section className="min-w-0 rounded-xl border border-border bg-background/40 p-4" aria-labelledby="working-folder-heading">
          <p id="working-folder-heading" className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Working folder</p>
          <p className="mt-2 break-all font-mono text-sm" title={model.workingFolder}>{model.workingFolder}</p>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">This stable path follows the version opened in Mesh.</p>
          <div className="mt-4 flex flex-wrap gap-2">
            {action("open-folder", "primary")}
            {action("copy-working-path", "quiet")}
            {action("open-version", "secondary")}
          </div>
        </section>

        <section className="min-w-0 rounded-xl border border-border bg-background/40 p-4" aria-labelledby="agent-folder-heading">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p id="agent-folder-heading" className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">{model.agentFolderLabel}</p>
            <Badge tone={model.agentAssigned ? "warning" : "neutral"}>{model.agentAssigned ? "In custody" : "Ready"}</Badge>
          </div>
          <p className="mt-2 break-all font-mono text-sm" title={model.agentFolder}>{model.agentFolder}</p>
          <p className="mt-3 text-sm leading-6 text-muted-foreground">{model.nativeFolderHint}</p>
          <div className="mt-4 flex flex-wrap gap-2">
            {action("start-codex", "primary")}
            {action("start-agent-copy", "secondary")}
            {action("open-terminal", "secondary")}
            {action("copy-agent-path", "quiet")}
            {action("finish-agent", "quiet")}
          </div>
        </section>
      </div>

      <section className="min-w-0 rounded-xl border border-border bg-muted/20 p-4" aria-labelledby="destination-heading">
        <p id="destination-heading" className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Original or destination folder</p>
        <p className="mt-2 break-all font-mono text-sm" title={model.destination}>{model.destination}</p>
        <p className="mt-2 text-sm text-muted-foreground">Mesh previews and rechecks saved content before any destination write.</p>
        <div className="mt-4">{action("update-destination", "secondary")}</div>
      </section>

      <dl className="grid gap-3 text-sm sm:grid-cols-3">
        <Fact label="Private version" value={model.privateVersion} title={model.privateVersionTitle} />
        <Fact label="Shared version" value={model.sharedVersion} title={model.sharedVersionTitle} />
        <Fact label="Files and folders" value={String(model.entryCount)} />
      </dl>

      <div className="grid gap-4 lg:grid-cols-2">
        <details className="rounded-xl border border-border bg-background/40 p-4">
          <summary className="min-h-11 cursor-pointer font-semibold">Materialized paths ({model.entries.length})</summary>
          <ul className="mt-3 max-h-64 space-y-2 overflow-y-auto text-sm">
            {model.entries.map((entry, index) => <li className="break-all font-mono" key={`${index}:${entry}`}>{entry}</li>)}
          </ul>
        </details>
        <details className="rounded-xl border border-border bg-background/40 p-4" open={model.conditions.length > 0}>
          <summary className="min-h-11 cursor-pointer font-semibold">Conditions and unavailable controls ({model.conditions.length})</summary>
          {model.conditions.length ? (
            <ul className="mt-3 max-h-64 space-y-2 overflow-y-auto text-sm text-muted-foreground">
              {model.conditions.map((condition, index) => <li key={`${index}:${condition}`}>{condition}</li>)}
            </ul>
          ) : <p className="mt-3 text-sm text-muted-foreground">No reported conditions.</p>}
        </details>
      </div>

      <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
        <div>
          <p className="font-semibold">Support and recovery</p>
          <p className="text-sm text-muted-foreground">Diagnostics exclude file contents and paths. Rollback succeeds only while the managed copy matches its receipt.</p>
        </div>
        <div className="flex flex-wrap gap-2">
          {action("copy-diagnostics", "quiet")}
          {action("rollback", "danger")}
        </div>
      </footer>
    </div>
  );
}

function Fact({ label, value, title = value }: { label: string; value: string; title?: string }) {
  return (
    <div className="min-w-0 rounded-lg border border-border bg-background/40 p-3">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-1 truncate font-medium" title={title}>{value}</dd>
    </div>
  );
}

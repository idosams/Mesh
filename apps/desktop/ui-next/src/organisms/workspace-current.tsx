import { useTranslation } from "../lib/localization";
import { useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import type {
  WorkspaceCurrentActionId,
  WorkspaceCurrentIntent,
  WorkspaceCurrentModel,
} from "../models/workspace-current";
import {
  CURRENT_CONDITION_ROW_LIMIT,
  CURRENT_DETAIL_ROW_LIMIT,
  CURRENT_MONITOR_ROW_LIMIT,
  workspaceCurrentListProjection,
  workspaceCurrentWorkspaceProjection,
} from "../models/workspace-current";

type IntentHandler = (intent: WorkspaceCurrentIntent) => void;

export function WorkspaceCurrent({ model, generation, onIntent }: {
  model: WorkspaceCurrentModel;
  generation: number;
  onIntent: IntentHandler;
}) {
  const [workspaceQuery, setWorkspaceQuery] = useState("");
  return (
    <WorkspaceCurrentView
      model={model}
      generation={generation}
      onIntent={onIntent}
      workspaceQuery={workspaceQuery}
      onWorkspaceQueryChange={setWorkspaceQuery}
    />
  );
}

export function WorkspaceCurrentView({ model, generation, onIntent, workspaceQuery, onWorkspaceQueryChange }: {
  model: WorkspaceCurrentModel;
  generation: number;
  onIntent: IntentHandler;
  workspaceQuery: string;
  onWorkspaceQueryChange: (query: string) => void;
}) {
  const t = useTranslation();
  const workspaceProjection = workspaceCurrentWorkspaceProjection(model.workspaces, workspaceQuery);
  const agentChangeProjection = workspaceCurrentListProjection(model.agentActivity.changes, CURRENT_MONITOR_ROW_LIMIT);
  const entryProjection = workspaceCurrentListProjection(model.entries, CURRENT_DETAIL_ROW_LIMIT);
  const conditionProjection = workspaceCurrentListProjection(model.conditions, CURRENT_CONDITION_ROW_LIMIT);
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
        {t(item.label)}
      </Button>
    ) : null;
  };

  return (
    <div data-mesh-proof="current-mounted" data-mesh-generation={generation} aria-label={t("Current workspace details")} className="grid gap-5 p-5 lg:p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">{t("Workspace details")}</p>
            <Badge tone={model.state === "Needs attention" ? "warning" : model.state === "Working" ? "neutral" : "positive"}>
              {model.state}
            </Badge>
            <span data-mesh-proof={model.agentAssigned ? "current-agent-assigned" : "current-agent-available"}>
              {model.agentAssigned ? <Badge tone="warning">{t("Agent folder assigned")}</Badge> : <Badge tone="neutral">{t("Agent folder available")}</Badge>}
            </span>
          </div>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">{t("Current workspace")}</h2>
          <p className="mt-1 text-sm text-muted-foreground">{model.recordSummary}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          {action("return-workspace", "quiet")}
          {action("refresh", "quiet")}
        </div>
      </header>

      <details className="rounded-xl border border-border bg-background/40" aria-labelledby="workspace-switcher-heading">
        <summary className="flex min-h-12 cursor-pointer items-center justify-between gap-3 px-4 py-3 font-semibold">
          <span>{t("Switch workspace")}</span>
          <span className="text-xs font-normal text-muted-foreground">{model.workspaces.length.toLocaleString()} {t("recent")}</span>
        </summary>
        <div className="border-t border-border p-4">
          <div className="flex flex-wrap items-end justify-between gap-2">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">{t("Workspaces and agents")}</p>
            <h3 id="workspace-switcher-heading" className="mt-1 font-semibold">{t("Switch without losing agent context")}</h3>
          </div>
          </div>
        <label className="mt-3 grid max-w-xl gap-1.5 text-xs font-medium text-muted-foreground">
          <span>{t("Find a workspace or agent")}</span>
          <input
            type="search" dir="auto"
            value={workspaceQuery}
            onChange={(event) => onWorkspaceQueryChange(event.currentTarget.value)}
            placeholder={t("Search by name, state, or path…")}
            className="min-h-11 rounded-lg border border-border bg-background px-3 text-sm text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
          />
        </label>
        <p className="mt-2 text-xs text-muted-foreground">
          {workspaceProjection.truncated
            ? `${workspaceProjection.items.length.toLocaleString()} of ${workspaceProjection.matched.toLocaleString()} matching workspaces shown`
            : `${workspaceProjection.matched.toLocaleString()} matching workspaces`}
        </p>
        <div className="mt-3 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
          {workspaceProjection.items.map((workspace) => (
            <button
              key={workspace.path}
              type="button"
              className="min-h-14 rounded-lg border border-border bg-muted/20 px-3 py-2 text-left hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-65"
              disabled={!workspace.canOpen}
              aria-current={workspace.state === "current" ? "true" : undefined}
              title={workspace.path}
              onClick={() => onIntent({ type: "switch-workspace", path: workspace.path })}
              data-mesh-current-workspace={workspace.path}
            >
              <span className="block truncate font-medium">{workspace.label}</span>
              <span className="mt-1 block text-xs text-muted-foreground">
                {workspace.state === "current" ? t("Current workspace") : workspace.state === "agent-assigned" ? t("Agent running") : t("Available")}
              </span>
            </button>
          ))}
        </div>
        {workspaceProjection.truncated ? (
          <p className="mt-3 rounded-lg border border-amber-400/25 bg-amber-400/5 p-3 text-xs leading-5 text-amber-100" role="status">
            {t("Showing the first")}{" "}{workspaceProjection.items.length.toLocaleString()} {t("matching workspaces. Refine the search to reach a narrower result set.")}</p>
        ) : null}
        {workspaceProjection.matched === 0 ? <p className="mt-3 text-sm text-muted-foreground">{t("No workspace or agent matches this search.")}</p> : null}
        </div>
      </details>

      <section className="rounded-xl border border-border bg-muted/20 p-4" aria-labelledby="live-agent-heading">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">{t("Read-only monitor")}</p>
            <h3 id="live-agent-heading" className="mt-1 font-semibold">{t("Live agent work")}</h3>
          </div>
          <Badge tone={model.agentActivity.state === "error" ? "warning" : model.agentAssigned ? "positive" : "neutral"}>
            {model.agentActivity.state === "scanning" ? t("Checking") : model.agentAssigned ? t("Monitoring") : t("No active agent")}
          </Badge>
        </div>
        <p className="mt-2 text-sm leading-6 text-muted-foreground" role="status" aria-live="polite">
          {model.agentActivity.summary}
        </p>
        {model.agentActivity.changes.length ? (
          <ul className="mt-3 max-h-56 space-y-2 overflow-auto" data-mesh-live-agent-work="true">
            {agentChangeProjection.items.map((change) => (
              <li data-mesh-live-change="true" key={`${change.kind}:${change.path}`} className="flex min-h-10 items-center justify-between gap-3 rounded-lg border border-border bg-background/60 px-3 py-2 text-sm">
                <span className="min-w-0 break-all font-mono">{change.path}</span>
                <span className="shrink-0 text-xs text-muted-foreground">{change.kind.replaceAll("-", " ")}</span>
              </li>
            ))}
            {agentChangeProjection.truncated ? (
              <li className="rounded-lg border border-amber-400/25 bg-amber-400/5 p-3 text-xs leading-5 text-amber-100" role="status">
                {t("Showing the first")}{" "}{agentChangeProjection.items.length.toLocaleString()} {t("of")}{" "}{agentChangeProjection.matched.toLocaleString()} {t("live changes. Open Live agent work in Review and filter to inspect a narrower result set.")}</li>
            ) : null}
          </ul>
        ) : null}
        {model.agentAssigned ? (
          <p className="mt-3 text-xs text-muted-foreground">{t("Monitoring never saves or approves work. Finish agent handoff still performs the authoritative complete scan before the work enters private history and Review.")}</p>
        ) : null}
      </section>

      <div className="grid gap-4 xl:grid-cols-2">
        <section className="min-w-0 rounded-xl border border-border bg-background/40 p-4" aria-labelledby="working-folder-heading">
          <p id="working-folder-heading" className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">{t("Working folder")}</p>
          <p className="mt-2 break-all font-mono text-sm" title={model.workingFolder}>{model.workingFolder}</p>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">{t("This stable path follows the version opened in Mesh.")}</p>
          <div className="mt-4 flex flex-wrap gap-2">
            {action("open-folder", "primary")}
            {action("copy-working-path", "quiet")}
            {action("open-version", "secondary")}
          </div>
        </section>

        <section className="min-w-0 rounded-xl border border-border bg-background/40 p-4" aria-labelledby="agent-folder-heading">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p id="agent-folder-heading" className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">{t(model.agentFolderLabel)}</p>
            <Badge tone={model.agentAssigned ? "warning" : "neutral"}>{model.agentAssigned ? t("In custody") : t("Ready")}</Badge>
          </div>
          <p className="mt-2 break-all font-mono text-sm" title={model.agentFolder}>{model.agentFolder}</p>
          <p className="mt-3 text-sm leading-6 text-muted-foreground">{t(model.nativeFolderHint)}</p>
          <div className="mt-4 flex flex-wrap gap-2">
            {action("start-codex", "primary")}
            {action("start-agent-copy", "secondary")}
            {action("open-terminal", "secondary")}
            {action("copy-agent-path", "quiet")}
            {action("finish-agent", "quiet")}
          </div>
        </section>
      </div>

      <details className="rounded-xl border border-border bg-background/30">
        <summary className="min-h-12 cursor-pointer px-4 py-3 font-semibold">{t("Workspace details and recovery")}</summary>
        <div className="grid gap-4 border-t border-border p-4">
          <section className="min-w-0 rounded-xl border border-border bg-muted/20 p-4" aria-labelledby="destination-heading">
            <p id="destination-heading" className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">{t("Original or destination folder")}</p>
            <p className="mt-2 break-all font-mono text-sm" title={model.destination}>{model.destination}</p>
            <p className="mt-2 text-sm text-muted-foreground">{t("Mesh previews and rechecks saved content before any destination write.")}</p>
            <div className="mt-4">{action("update-destination", "secondary")}</div>
          </section>

          <dl className="grid gap-3 text-sm sm:grid-cols-3">
            <Fact label={t("Private version")} value={model.privateVersion} title={model.privateVersionTitle} />
            <Fact label={t("Shared version")} value={model.sharedVersion} title={model.sharedVersionTitle} />
            <Fact label={t("Files and folders")} value={String(model.entryCount)} />
          </dl>

          <div className="grid gap-4 lg:grid-cols-2">
            <details className="rounded-xl border border-border bg-background/40 p-4">
          <summary className="min-h-11 cursor-pointer font-semibold">{t("Materialized paths (")}{model.entries.length})</summary>
          <ul className="mt-3 max-h-64 space-y-2 overflow-y-auto text-sm">
            {entryProjection.items.map((entry, index) => <li data-mesh-materialized-entry="true" className="break-all font-mono" key={`${index}:${entry}`}>{entry}</li>)}
            {entryProjection.truncated ? <li className="text-xs text-muted-foreground">{t("Showing the first")}{" "}{entryProjection.items.length.toLocaleString()} {t("of")}{" "}{entryProjection.matched.toLocaleString()} {t("paths. Use Files to search the complete workspace.")}</li> : null}
          </ul>
            </details>
            <details className="rounded-xl border border-border bg-background/40 p-4" open={model.conditions.length > 0}>
          <summary className="min-h-11 cursor-pointer font-semibold">{t("Conditions and unavailable controls (")}{model.conditions.length})</summary>
          {model.conditions.length ? (
            <ul className="mt-3 max-h-64 space-y-2 overflow-y-auto text-sm text-muted-foreground">
              {conditionProjection.items.map((condition, index) => <li data-mesh-current-condition="true" key={`${index}:${condition}`}>{condition}</li>)}
              {conditionProjection.truncated ? <li>{t("Showing the first")}{" "}{conditionProjection.items.length.toLocaleString()} {t("of")}{" "}{conditionProjection.matched.toLocaleString()} {t("conditions.")}</li> : null}
            </ul>
          ) : <p className="mt-3 text-sm text-muted-foreground">{t("No reported conditions.")}</p>}
            </details>
          </div>

          <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
            <div>
              <p className="font-semibold">{t("Support and recovery")}</p>
              <p className="text-sm text-muted-foreground">{t("Diagnostics exclude file contents and paths. Rollback succeeds only while the managed copy matches its receipt.")}</p>
            </div>
            <div className="flex flex-wrap gap-2">
              {action("copy-diagnostics", "quiet")}
              {action("rollback", "danger")}
            </div>
          </footer>
        </div>
      </details>
    </div>
  );
}

function Fact({ label, value, title = value }: { label: string; value: string; title?: string }) {
  const t = useTranslation();
  return (
    <div className="min-w-0 rounded-lg border border-border bg-background/40 p-3">
      <dt className="text-xs text-muted-foreground">{t(label)}</dt>
      <dd className="mt-1 truncate font-medium" title={t(title)}>{value}</dd>
    </div>
  );
}

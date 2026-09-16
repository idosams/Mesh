import { useEffect, useRef, useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { rovingSelectionIndex } from "../models/roving-selection";
import type {
  WorkspaceVersionItem,
  WorkspaceVersionsIntent,
  WorkspaceVersionsModel,
} from "../models/workspace-versions";

function relationBadge(item: WorkspaceVersionItem) {
  if (item.relation === "current") return <Badge tone="positive">Current</Badge>;
  if (item.relation === "concurrent") return <Badge tone="warning">Concurrent</Badge>;
  return <Badge tone="neutral">Earlier</Badge>;
}

export function WorkspaceVersionNavigator({ model, generation, onIntent }: {
  model: WorkspaceVersionsModel;
  generation: number;
  onIntent: (intent: WorkspaceVersionsIntent) => void;
}) {
  const versionButtons = useRef(new Map<string, HTMLButtonElement>());
  const [customLocation, setCustomLocation] = useState(model.customLocation);
  useEffect(() => setCustomLocation(model.customLocation), [generation, model.customLocation]);
  const selectedIndex = model.selectedOperation === null
    ? -1
    : model.versions.findIndex((item) => item.operation === model.selectedOperation);
  const changeTitle = model.changeBasis === "initial"
    ? "Initial saved contents"
    : model.changeBasis === "previous-point"
      ? `What changed since point ${model.basisOrdinal}`
      : model.changeBasis === "combined-history"
        ? "Combined saved contents"
        : "What changed";
  const emptyChangeExplanation = model.changeBasis === "combined-history"
    ? "This point combines concurrent saved work. Inspect the complete file list below."
    : "No visible file or folder changes.";
  const selectAt = (index: number) => {
    const candidate = model.versions[index];
    if (!candidate) return;
    versionButtons.current.get(candidate.operation)?.focus({ preventScroll: true });
    onIntent({ type: "select-version", operation: candidate.operation });
  };
  const moveSelection = (event: React.KeyboardEvent<HTMLButtonElement>, index: number) => {
    const destination = rovingSelectionIndex(model.versions.length, index, event.key, "all-wrap");
    if (destination === null) return;
    event.preventDefault();
    if (destination === index) return;
    selectAt(destination);
  };
  return (
    <section className="grid gap-5" aria-label="Workspace versions" data-mesh-proof="workspace-versions">
      <header>
        <div className="flex flex-wrap items-center gap-2">
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Workspace versions</p>
          <Badge tone={model.historyMode === "concurrent" ? "warning" : "neutral"}>
            {model.historyMode === "concurrent" ? "Concurrent history" : `${model.versions.length} saved point${model.versions.length === 1 ? "" : "s"}`}
          </Badge>
        </div>
        <h3 className="mt-2 text-xl font-semibold">Open an exact saved point</h3>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          Mesh verifies the complete point before it can open an independent native folder. The workspace you leave stays untouched.
        </p>
      </header>
      <div className="grid gap-4 lg:grid-cols-[minmax(14rem,0.58fr)_minmax(0,1fr)]">
        <div className="min-w-0 rounded-xl border border-border bg-muted/20 p-2">
          <div role="radiogroup" aria-label="Saved workspace points" className="grid max-h-72 gap-1 overflow-auto">
            {model.versions.map((item, index) => {
              const selected = item.operation === model.selectedOperation;
              return (
                <button
                  key={item.operation}
                  ref={(node) => {
                    if (node) versionButtons.current.set(item.operation, node);
                    else versionButtons.current.delete(item.operation);
                  }}
                  type="button"
                  role="radio"
                  data-mesh-proof="workspace-version-choice"
                  data-mesh-version-operation={item.operation}
                  disabled={!model.canSelect}
                  aria-checked={selected}
                  tabIndex={selected || (selectedIndex < 0 && index === 0) ? 0 : -1}
                  className="flex min-h-12 w-full items-center justify-between gap-3 rounded-lg border border-transparent px-3 py-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring aria-checked:border-primary aria-checked:bg-primary/10"
                  onClick={() => onIntent({ type: "select-version", operation: item.operation })}
                  onKeyDown={(event) => moveSelection(event, index)}
                >
                  <span className="min-w-0">
                    <span className="block truncate font-medium">{item.label}</span>
                    <span className="block font-mono text-xs text-muted-foreground">{item.operation.slice(0, 12)}…</span>
                  </span>
                  {relationBadge(item)}
                </button>
              );
            })}
          </div>
        </div>
        <section
          className="min-w-0 rounded-xl border border-border bg-background/35 p-4"
          aria-label="Selected version preview"
          aria-busy={model.previewState === "loading"}
        >
          <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
            {model.previewTitle}. {model.previewSummary}
          </p>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">Verified preview</p>
          <h4 className="mt-2 text-lg font-semibold">{model.previewTitle}</h4>
          <p
            className="mt-2 text-sm leading-6 text-muted-foreground"
            data-mesh-proof={model.previewState === "error" ? "workspace-version-preview-error" : undefined}
          >
            {model.previewSummary}
          </p>
          {model.previewState === "ready" ? (
            <div className="mt-4 grid gap-4 sm:grid-cols-2" data-mesh-proof="workspace-version-preview-ready">
              <VersionList title={changeTitle} lines={model.changes} empty={emptyChangeExplanation} />
              <VersionList title="Files in this point" lines={model.entries} empty="This saved point contains no files or folders." />
            </div>
          ) : null}
        </section>
      </div>
      <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:justify-end">
        {model.canUseCustomLocation ? (
          <label className="grid min-w-0 flex-1 gap-2 text-sm font-medium" htmlFor="workspace-version-custom-location">
            Custom private location
            <input
              id="workspace-version-custom-location"
              autoComplete="off"
              className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
              placeholder="Leave blank for Mesh-managed storage"
              value={customLocation}
              onInput={(event) => {
                const path = event.currentTarget.value;
                setCustomLocation(path);
                onIntent({ type: "set-custom-location", path });
              }}
            />
          </label>
        ) : null}
        <Button
          variant="secondary"
          disabled={!model.canStartCodex || model.selectedOperation === null}
          onClick={() => model.selectedOperation && onIntent({ type: "start-codex", operation: model.selectedOperation })}
        >
          {model.codexLabel}
        </Button>
        <Button
          variant="primary"
          data-mesh-proof="workspace-version-open"
          disabled={!model.canOpen || model.selectedOperation === null}
          onClick={() => model.selectedOperation && onIntent({ type: "open-version", operation: model.selectedOperation })}
        >
          {model.openLabel}
        </Button>
      </div>
      <p className="text-xs leading-5 text-muted-foreground">
        Existing editors and agents keep their current directory handle. Reopen them only when they should follow the newly selected working folder.
      </p>
    </section>
  );
}

function VersionList({ title, lines, empty }: { title: string; lines: readonly string[]; empty: string }) {
  return (
    <div className="min-w-0">
      <h5 className="text-sm font-semibold">{title}</h5>
      <ul className="mt-2 max-h-44 overflow-auto rounded-lg border border-border bg-muted/15 p-3 text-xs leading-6">
        {lines.length > 0
          ? lines.map((line, index) => <li key={`${index}:${line}`} className="break-all">{line}</li>)
          : <li className="text-muted-foreground">{empty}</li>}
      </ul>
    </div>
  );
}

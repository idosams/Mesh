import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { RestorePreviewCard } from "../molecules/restore-preview-card";
import type { WorkspaceRestoreIntent, WorkspaceRestoreModel } from "../models/workspace-restore";

export function WorkspaceRestore({ model, onIntent }: Readonly<{
  model: WorkspaceRestoreModel;
  onIntent: (intent: WorkspaceRestoreIntent) => void;
}>) {
  const selectedFile = model.files.find((choice) => choice.id === model.selectedFileId) || null;
  return (
    <div aria-label="Restore an earlier file version" className="grid gap-5 p-5 lg:p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="max-w-3xl">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">History</p>
            <Badge tone="warning">Working copy only</Badge>
          </div>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">Restore an earlier file version</h2>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">Choose a file and a saved version, then inspect the change before replacing the file in your working folder. Private history is not rewritten.</p>
        </div>
        <Button variant="secondary" disabled={!model.canUndo} onClick={() => onIntent({ type: "undo" })}>{model.undoLabel}</Button>
      </header>

      <div className="grid gap-4 lg:grid-cols-2">
        <label className="grid gap-2 text-sm font-semibold" htmlFor="restore-next-file">
          File
          <select
            id="restore-next-file"
            value={model.selectedFileId}
            disabled={!model.canSelectFile}
            className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            onChange={(event) => onIntent({ type: "select-file", id: event.currentTarget.value })}
          >
            <option value="" disabled={model.files.length > 0}>{model.files.length ? "Choose a file" : "No retained file history"}</option>
            {model.files.map((choice) => <option key={choice.id} value={choice.id}>{choice.label}</option>)}
          </select>
        </label>
        <label className="grid gap-2 text-sm font-semibold" htmlFor="restore-next-version">
          Earlier saved version
          <select
            id="restore-next-version"
            value={model.selectedVersionId}
            disabled={!model.canSelectVersion}
            aria-describedby="restore-next-hint"
            className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            onChange={(event) => onIntent({ type: "select-version", id: event.currentTarget.value })}
          >
            <option value="" disabled={model.versions.length > 0}>{selectedFile ? (model.versions.length ? "Choose a saved version" : "No earlier version retained") : "Choose a file first"}</option>
            {model.versions.map((choice) => <option key={choice.id} value={choice.id}>{choice.label}</option>)}
          </select>
        </label>
      </div>

      {selectedFile ? (
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm">
          <Badge tone="neutral">{selectedFile.format}</Badge>
          <span className="break-all font-mono" title={selectedFile.path}>{selectedFile.path}</span>
        </div>
      ) : null}
      <p id="restore-next-hint" className="text-sm leading-6 text-muted-foreground" aria-live="polite">{model.hint}</p>

      {model.preview ? <RestorePreviewCard preview={model.preview} canApply={model.canApply} /> : (
        <div className="rounded-xl border border-dashed border-border p-5 text-sm leading-6 text-muted-foreground">
          Select an earlier saved version and preview it. Mesh will verify the exact retained bytes and the current working file before Restore becomes available.
        </div>
      )}

      <footer className="flex flex-col gap-3 border-t border-border pt-4 sm:flex-row sm:items-center sm:justify-end">
        <Button className="sm:min-w-36" variant="secondary" disabled={!model.canPreview} onClick={() => onIntent({ type: "preview" })}>Preview restore</Button>
        <Button className="sm:min-w-48" variant="primary" disabled={!model.canApply} onClick={() => onIntent({ type: "apply" })}>Restore in working copy</Button>
      </footer>
    </div>
  );
}

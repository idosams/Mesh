import { useTranslation } from "../lib/localization";
import { useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { RestorePreviewCard } from "../molecules/restore-preview-card";
import {
  workspaceRestoreFileProjection,
  workspaceRestoreVersionProjection,
  type WorkspaceRestoreIntent,
  type WorkspaceRestoreModel,
} from "../models/workspace-restore";

export function WorkspaceRestore({ model, onIntent }: Readonly<{
  model: WorkspaceRestoreModel;
  onIntent: (intent: WorkspaceRestoreIntent) => void;
}>) {
  const t = useTranslation();
  const [fileQuery, setFileQuery] = useState("");
  const [versionQuery, setVersionQuery] = useState("");
  const selectedFile = model.files.find((choice) => choice.id === model.selectedFileId) || null;
  const fileProjection = workspaceRestoreFileProjection(model.files, fileQuery, model.selectedFileId);
  const versionProjection = workspaceRestoreVersionProjection(model.versions, versionQuery, model.selectedVersionId);
  return (
    <div aria-label={t("Restore an earlier file version")} className="grid gap-5 p-5 lg:p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="max-w-3xl">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">{t("History")}</p>
            <Badge tone="warning">{t("Working copy only")}</Badge>
          </div>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">{t("Restore an earlier file version")}</h2>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">{t("Choose a file and a saved version, then inspect the change before replacing the file in your working folder. Private history is not rewritten.")}</p>
        </div>
        <Button variant="secondary" disabled={!model.canUndo} onClick={() => onIntent({ type: "undo" })}>{t(model.undoLabel)}</Button>
      </header>

      <div className="grid gap-4 lg:grid-cols-2">
        <div className="grid gap-2 text-sm font-semibold">
          <label htmlFor="restore-file-filter">{t("Find a retained file")}</label>
          <input id="restore-file-filter" type="search" dir="auto" value={fileQuery} onChange={(event) => setFileQuery(event.currentTarget.value)} placeholder={t("Filter retained files…")} className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring" />
          <label htmlFor="restore-next-file">{t("File")}</label>
          <select
            id="restore-next-file"
            value={model.selectedFileId}
            disabled={!model.canSelectFile}
            className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            onChange={(event) => onIntent({ type: "select-file", id: event.currentTarget.value })}
          >
            <option value="" disabled={model.files.length > 0}>{model.files.length ? t("Choose a file") : t("No retained file history")}</option>
            {fileProjection.items.map((choice) => <option data-mesh-restore-file="true" key={choice.id} value={choice.id}>{choice.label}</option>)}
          </select>
          <span className="text-xs font-normal text-muted-foreground">{restoreProjectionCopy(fileProjection, "retained files")}</span>
        </div>
        <div className="grid gap-2 text-sm font-semibold">
          <label htmlFor="restore-version-filter">{t("Find an earlier saved version")}</label>
          <input id="restore-version-filter" type="search" dir="auto" value={versionQuery} onChange={(event) => setVersionQuery(event.currentTarget.value)} placeholder={t("Filter saved versions…")} className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring" />
          <label htmlFor="restore-next-version">{t("Earlier saved version")}</label>
          <select
            id="restore-next-version"
            value={model.selectedVersionId}
            disabled={!model.canSelectVersion}
            aria-describedby="restore-next-hint"
            className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            onChange={(event) => onIntent({ type: "select-version", id: event.currentTarget.value })}
          >
            <option value="" disabled={model.versions.length > 0}>{selectedFile ? (model.versions.length ? "Choose a saved version" : "No earlier version retained") : t("Choose a file first")}</option>
            {versionProjection.items.map((choice) => <option data-mesh-restore-version="true" key={choice.id} value={choice.id}>{choice.label}</option>)}
          </select>
          <span className="text-xs font-normal text-muted-foreground">{restoreProjectionCopy(versionProjection, "saved versions")}</span>
        </div>
      </div>

      {selectedFile ? (
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm">
          <Badge tone="neutral">{selectedFile.format}</Badge>
          <span className="break-all font-mono" title={selectedFile.path}>{selectedFile.path}</span>
        </div>
      ) : null}
      <p id="restore-next-hint" className="text-sm leading-6 text-muted-foreground" aria-live="polite">{t(model.hint)}</p>

      {model.preview ? <RestorePreviewCard preview={model.preview} canApply={model.canApply} /> : (
        <div className="rounded-xl border border-dashed border-border p-5 text-sm leading-6 text-muted-foreground">
          {t("Select an earlier saved version and preview it. Mesh will verify the exact retained bytes and the current working file before Restore becomes available.")}</div>
      )}

      <footer className="flex flex-col gap-3 border-t border-border pt-4 sm:flex-row sm:items-center sm:justify-end">
        <Button className="sm:min-w-36" variant="secondary" disabled={!model.canPreview} onClick={() => onIntent({ type: "preview" })}>{t("Preview restore")}</Button>
        <Button className="sm:min-w-48" variant="primary" disabled={!model.canApply} onClick={() => onIntent({ type: "apply" })}>{t("Restore in working copy")}</Button>
      </footer>
    </div>
  );
}

function restoreProjectionCopy(
  projection: Readonly<{
    items: readonly unknown[];
    matched: number;
    retainedSelected: boolean;
    truncated: boolean;
  }>,
  label: string,
): string {
  if (projection.retainedSelected) {
    return `${projection.matched.toLocaleString()} matching ${label}; the current choice remains available.`;
  }
  if (projection.truncated) {
    return `${projection.items.length.toLocaleString()} of ${projection.matched.toLocaleString()} matching ${label} shown.`;
  }
  return `${projection.matched.toLocaleString()} matching ${label}.`;
}

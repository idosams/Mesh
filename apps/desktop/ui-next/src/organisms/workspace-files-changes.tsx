import { useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { SegmentedControl } from "../atoms/segmented-control";
import type {
  WorkspaceFilesChangesIntent,
  WorkspaceFilesChangesModel,
  WorkspaceWorkActionId,
} from "../models/workspace-files-changes";

type WorkbenchProps = {
  model: WorkspaceFilesChangesModel;
  onIntent: (intent: WorkspaceFilesChangesIntent) => void;
};

function actionMap(model: WorkspaceFilesChangesModel) {
  return new Map(model.actions.map((action) => [action.id, action]));
}

function WorkAction({ id, actions, onIntent, variant = "secondary", activationEcho, activationIntent, enabledOverride }: {
  id: WorkspaceWorkActionId;
  actions: ReturnType<typeof actionMap>;
  onIntent: WorkbenchProps["onIntent"];
  variant?: "primary" | "secondary" | "quiet" | "danger";
  activationEcho?: () => null | Readonly<{ field: "newPath" | "movePath" | "selectedFile" | "editorText"; value: string }>;
  activationIntent?: () => WorkspaceFilesChangesIntent;
  enabledOverride?: boolean;
}) {
  const action = actions.get(id);
  if (!action) return null;
  return (
    <Button variant={variant} disabled={enabledOverride === undefined ? !action.enabled : !enabledOverride} onClick={() => {
      const exactIntent = activationIntent?.();
      const echo = exactIntent ? null : activationEcho?.();
      onIntent(exactIntent || (echo ? { type: "activate", action: id, ...echo } as WorkspaceFilesChangesIntent : { type: "activate", action: id }));
    }} data-mesh-work-action={id}>
      {action.label}
    </Button>
  );
}

export function WorkspaceFiles({ model, onIntent }: WorkbenchProps) {
  const actions = useMemo(() => actionMap(model), [model]);
  const newPathRef = useRef<HTMLInputElement>(null);
  const movePathRef = useRef<HTMLInputElement>(null);
  return (
    <div className="grid gap-5 p-5 lg:p-6" aria-label="Files and folders">
      <header>
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Files</p>
        <h2 className="mt-2 text-xl font-semibold tracking-tight">Create and organize workspace entries</h2>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          Accepted changes are signed and added to private history before Mesh reports them saved.
        </p>
      </header>
      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-xl border border-border bg-background/40 p-4" aria-labelledby="new-entry-heading">
          <h3 id="new-entry-heading" className="font-semibold">New entry</h3>
          <label className="mt-3 grid gap-2 text-sm font-medium">
            Relative path
            <input
              ref={newPathRef}
              className="min-h-11 rounded-lg border border-border bg-background px-3 font-mono outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              value={model.files.newPath}
              disabled={!model.files.canEditNewPath}
              placeholder="notes/idea.txt"
              onChange={(event) => onIntent({ type: "set-field", field: "newPath", value: event.currentTarget.value })}
            />
          </label>
          <div className="mt-4 flex flex-wrap gap-2">
            <WorkAction id="create-text" actions={actions} onIntent={onIntent} variant="primary" activationEcho={() => ({ field: "newPath", value: newPathRef.current?.value ?? model.files.newPath })} />
            <WorkAction id="create-folder" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "newPath", value: newPathRef.current?.value ?? model.files.newPath })} />
          </div>
        </section>
        <section className="rounded-xl border border-border bg-background/40 p-4" aria-labelledby="existing-entry-heading">
          <h3 id="existing-entry-heading" className="font-semibold">Existing entry</h3>
          <label className="mt-3 grid gap-2 text-sm font-medium">
            File or folder
            <select
              className="min-h-11 rounded-lg border border-border bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              value={model.files.selectedEntry}
              disabled={!model.files.canSelectEntry}
              onChange={(event) => onIntent({ type: "set-field", field: "selectedEntry", value: event.currentTarget.value })}
            >
              <option value="">{model.files.entries.length ? "Choose an entry" : "No entries yet"}</option>
              {model.files.entries.map((entry) => <option key={entry.value} value={entry.value}>{entry.label}</option>)}
            </select>
          </label>
          <label className="mt-3 grid gap-2 text-sm font-medium">
            Move or rename to
            <input
              ref={movePathRef}
              className="min-h-11 rounded-lg border border-border bg-background px-3 font-mono outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              value={model.files.movePath}
              disabled={!model.files.canEditMovePath}
              placeholder="notes/final.txt"
              onChange={(event) => onIntent({ type: "set-field", field: "movePath", value: event.currentTarget.value })}
            />
          </label>
          <div className="mt-4 flex flex-wrap gap-2">
            <WorkAction id="move-entry" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "movePath", value: movePathRef.current?.value ?? model.files.movePath })} />
            <WorkAction id="delete-entry" actions={actions} onIntent={onIntent} variant="danger" />
          </div>
        </section>
      </div>
      <p className="rounded-lg border border-border bg-muted/20 p-3 text-sm text-muted-foreground" role="status" aria-live="polite">
        {model.files.status}
      </p>
      <p className="text-xs text-muted-foreground">Delete accepts files and empty folders. Saved file content remains retained in immutable history.</p>
    </div>
  );
}

export function WorkspaceChanges({ model, onIntent }: WorkbenchProps) {
  const actions = useMemo(() => actionMap(model), [model]);
  const [view, setView] = useState<"Edit" | "Inline" | "Split">("Edit");
  const [selectedFileDraft, setSelectedFileDraft] = useState(model.changes.selectedFile);
  const selectedFileRef = useRef<HTMLSelectElement>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const missingSourceRef = useRef<HTMLSelectElement>(null);
  const moveTargetRef = useRef<HTMLSelectElement>(null);
  useEffect(() => {
    setView("Edit");
    setSelectedFileDraft(model.changes.selectedFile);
  }, [model.changes.selectedFile]);
  const selectedFileCanLoad = model.changes.canSelectFile
    && selectedFileDraft.length > 0
    && model.changes.files.some((file) => file.value === selectedFileDraft);
  const editorVisible = model.changes.editorKind !== "none";
  return (
    <div className="grid gap-5 p-5 lg:p-6" aria-label="Changes and text editor">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Changes</p>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">Inspect, edit, and save private work</h2>
          <Badge tone="neutral">LOCAL + AUTHENTICATED</Badge>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">
            Edit here or work normally in the stable native folder with a local editor. Give Terminal or a long-running agent the independent agent folder shown under Current; it starts at the selected durable version and then remains writable even if Mesh switches elsewhere. Review-first mode keeps native changes visible until an exact inspected save completes. Returning to Mesh automatically inspects the selected folder; selecting a different recent agent folder inspects it immediately, while inactive agent folders are not continuously watched. Find folder changes retries the read explicitly.
          </p>
        </div>
        <WorkAction id="scan-files" actions={actions} onIntent={onIntent} />
      </header>

      <section className="rounded-xl border border-border bg-background/40 p-4" aria-labelledby="file-editor-heading">
        <h3 id="file-editor-heading" className="sr-only">File editor and comparison</h3>
        <div className="flex flex-wrap items-end justify-between gap-3">
          <label className="grid min-w-0 basis-full gap-2 text-sm font-medium sm:min-w-[16rem] sm:flex-1">
            File
            <select
              className="min-h-11 min-w-0 w-full rounded-lg border border-border bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              ref={selectedFileRef}
              value={selectedFileDraft}
              disabled={!model.changes.canSelectFile}
              onChange={(event) => {
                setSelectedFileDraft(event.currentTarget.value);
                onIntent({ type: "set-field", field: "selectedFile", value: event.currentTarget.value });
              }}
            >
              <option value="">{model.changes.files.length ? "Choose a native file" : "No files available"}</option>
              {model.changes.files.map((file) => <option key={file.value} value={file.value}>{file.label}</option>)}
            </select>
          </label>
          <WorkAction
            id="load-file"
            actions={actions}
            onIntent={onIntent}
            enabledOverride={selectedFileCanLoad}
            activationEcho={() => ({ field: "selectedFile", value: selectedFileRef.current?.value ?? selectedFileDraft })}
          />
        </div>

        {editorVisible ? (
          <div className="mt-4 grid gap-3">
            {model.changes.editorKind === "text" ? (
              <SegmentedControl
                label="Text workspace view"
                value={view}
                onChange={(value) => setView(value as "Edit" | "Inline" | "Split")}
                options={["Edit", "Inline", "Split"]}
              />
            ) : null}
            {model.changes.editorKind === "text" && view === "Edit" ? (
              <label className="grid gap-2 text-sm font-medium">
                Managed text file contents
                <textarea
                  ref={editorRef}
                  className="min-h-72 w-full resize-y rounded-lg border border-border bg-background p-3 font-mono text-sm leading-6 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                  spellCheck={false}
                  value={model.changes.editorText}
                  disabled={!model.changes.canEditText}
                  onChange={(event) => onIntent({ type: "set-field", field: "editorText", value: event.currentTarget.value })}
                />
              </label>
            ) : model.changes.editorKind === "text" ? (
              <TextComparison before={model.changes.baselineText} after={model.changes.editorText} split={view === "Split"} />
            ) : (
              <p className="rounded-lg border border-border bg-muted/20 p-4 text-sm text-muted-foreground">Binary or large content is inspected by exact identity and remains outside the text editor.</p>
            )}
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="text-sm" role="status" aria-live="polite">{model.changes.editState} <code>{model.changes.editVersion}</code></p>
              <div className="flex flex-wrap gap-2">
                <WorkAction id="preserve-edit" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "editorText", value: editorRef.current?.value ?? model.changes.editorText })} />
                <WorkAction id="save-private" actions={actions} onIntent={onIntent} variant="primary" />
              </div>
            </div>
          </div>
        ) : <p className="mt-4 text-sm text-muted-foreground">Choose a file, then inspect it before editing or saving.</p>}
      </section>

      <section className="rounded-xl border border-border bg-muted/20 p-4" aria-labelledby="native-queue-heading" data-mesh-native-queue tabIndex={-1}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <h3 id="native-queue-heading" className="font-semibold">Native change queue</h3>
            <Badge tone={model.changes.queue.length ? "warning" : "neutral"}>{model.changes.queueSummary}</Badge>
          </div>
          <WorkAction id="save-all-private" actions={actions} onIntent={onIntent} variant="primary" />
        </div>
        {model.changes.queue.length ? (
          <ul className="mt-3 max-h-64 space-y-2 overflow-y-auto text-sm">
            {model.changes.queue.map((line, index) => <li className="break-all" key={`${index}:${line}`}>{line}</li>)}
          </ul>
        ) : <p className="mt-3 text-sm text-muted-foreground">No inspected folder changes are waiting.</p>}
        {model.changes.structural ? (
          <div className="mt-4 grid gap-3 border-t border-border pt-4">
            <label className="grid gap-2 text-sm font-medium">Missing tracked file
              <select
                ref={missingSourceRef}
                className="min-h-11 rounded-lg border border-border bg-background px-3"
                value={model.changes.structural.missingSource}
                data-mesh-work-field="missingSource"
                disabled={!model.changes.structural.canChoose}
                onChange={(event) => onIntent({ type: "set-field", field: "missingSource", value: event.currentTarget.value })}
              >
                {model.changes.structural.missingSources.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}
              </select>
            </label>
            <label className="grid gap-2 text-sm font-medium">What happened?
              <select
                ref={moveTargetRef}
                className="min-h-11 rounded-lg border border-border bg-background px-3"
                value={model.changes.structural.moveTarget}
                data-mesh-work-field="moveTarget"
                disabled={!model.changes.structural.canChoose}
                onChange={(event) => onIntent({ type: "set-field", field: "moveTarget", value: event.currentTarget.value })}
              >
                <option value="">It was deleted</option>
                {model.changes.structural.moveTargets.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}
              </select>
            </label>
            <WorkAction
              id="record-structural-change"
              actions={actions}
              onIntent={onIntent}
              activationIntent={() => ({
                type: "activate",
                action: "record-structural-change",
                missingSource: missingSourceRef.current?.value ?? model.changes.structural?.missingSource ?? "",
                moveTarget: moveTargetRef.current?.value ?? model.changes.structural?.moveTarget ?? "",
              })}
            />
            <p className="text-sm text-muted-foreground">{model.changes.structural.hint}</p>
          </div>
        ) : null}
      </section>

      <label className="flex min-h-11 items-start gap-3 rounded-xl border border-border bg-background/40 p-4 text-sm">
        <input
          className="mt-1 size-4"
          type="checkbox"
          checked={model.changes.autoSaveChecked}
          disabled={!model.changes.autoSaveEnabled}
          onChange={(event) => onIntent({ type: "set-auto-save", checked: event.currentTarget.checked })}
        />
        <span><strong>Automatically save safe native edits privately</strong><span className="mt-1 block text-muted-foreground">{model.changes.autoSaveHint}</span></span>
      </label>
      <p className="text-xs leading-5 text-muted-foreground">
        Inspect file re-reads exact operating-system bytes; binary and large files stay outside the text editor but remain savable. Choose Save privately to sign and append one inspected file. Mesh reviews a complete new folder tree and admits its folders parent-first. Outside the confirmed agent-finish flow, saving remains an explicit authenticated action. Automatic private save uses the same complete scan, exact reinspection, signature, and post-save rescan as Save all privately. It pauses for deletions, renames, symbolic links, special entries, active agent handoffs, or files that keep changing. Mesh stops on the first mismatch.
      </p>
    </div>
  );
}

export function TextComparison({ before, after, split }: { before: string; after: string; split: boolean }) {
  if (!split) {
    return (
      <section className="min-w-0 overflow-hidden rounded-lg border border-border bg-background/50" aria-label="Inline text comparison">
        <h4 className="border-b border-border px-3 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Entire inspected text compared with current draft</h4>
        <div className="grid gap-px bg-border">
          <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words bg-red-950/10 p-3 text-sm leading-6"><span className="sr-only">Before inspected edit: </span><span className="select-none text-red-400" aria-hidden="true">− </span>{before || "(empty file)"}</pre>
          <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words bg-emerald-950/10 p-3 text-sm leading-6"><span className="sr-only">Current draft: </span><span className="select-none text-emerald-400" aria-hidden="true">+ </span>{after || "(empty file)"}</pre>
        </div>
      </section>
    );
  }
  return (
    <div className="grid gap-3 md:grid-cols-2" aria-label="Split text comparison">
      <TextPane label="Before inspected edit" text={before} tone="before" />
      <TextPane label="Current draft" text={after} tone="after" />
    </div>
  );
}

function TextPane({ label, text, tone }: { label: string; text: string; tone: "before" | "after" }) {
  return (
    <section className="min-w-0 rounded-lg border border-border bg-background/50" aria-label={label}>
      <h4 className="border-b border-border px-3 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{label}</h4>
      <pre className={`max-h-80 overflow-auto whitespace-pre-wrap break-words p-3 text-sm leading-6 ${tone === "before" ? "bg-red-950/10" : "bg-emerald-950/10"}`}>{text || "(empty file)"}</pre>
    </section>
  );
}

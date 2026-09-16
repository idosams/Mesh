import { useEffect, useRef, useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { DestinationPlan } from "../molecules/destination-plan";
import type { WorkspaceDestinationActionId, WorkspaceDestinationIntent, WorkspaceDestinationModel } from "../models/workspace-destination";

export function destinationDraftWasAccepted(
  submittedDestination: string | null,
  chooserRevisionAtStart: number | null,
  model: Pick<WorkspaceDestinationModel, "chooserRevision" | "destination">,
) {
  return (submittedDestination !== null && submittedDestination === model.destination)
    || (chooserRevisionAtStart !== null && chooserRevisionAtStart !== model.chooserRevision);
}

export function WorkspaceDestination({ model, generation, onIntent }: Readonly<{
  model: WorkspaceDestinationModel;
  generation: number;
  onIntent: (intent: WorkspaceDestinationIntent) => void;
}>) {
  const destinationDraftRef = useRef<HTMLInputElement>(null);
  const selectedFileRef = useRef<HTMLSelectElement>(null);
  const submittedDestinationRef = useRef<string | null>(null);
  const chooserRevisionAtStartRef = useRef<number | null>(null);
  const [draftPresent, setDraftPresent] = useState(false);
  const actions = new Map(model.actions.map((action) => [action.id, action]));
  const action = (id: WorkspaceDestinationActionId) => actions.get(id)!;
  const button = (id: WorkspaceDestinationActionId, variant: "primary" | "secondary", proof?: string) => {
    const item = action(id);
    const activate = () => {
      const destination = submittedDestinationRef.current ?? model.destination;
      if (id === "preview-single") {
        onIntent({
          type: "activate",
          action: "preview-single",
          selectedFile: selectedFileRef.current?.value ?? model.selectedFile,
          destination,
        });
        return;
      }
      if (id === "preview-all") {
        onIntent({ type: "activate", action: "preview-all", destination });
        return;
      }
      onIntent({ type: "activate", action: id });
    };
    return <Button data-mesh-proof={proof} data-mesh-transition-action={id === "preview-single" || id === "preview-all" ? "read-only-preview" : undefined} variant={variant} disabled={!item.enabled} onClick={activate}>{item.label}</Button>;
  };
  const clearDestinationDraft = () => {
    if (destinationDraftRef.current) destinationDraftRef.current.value = "";
    setDraftPresent(false);
  };
  const chooseDestination = () => {
    chooserRevisionAtStartRef.current = model.chooserRevision;
    onIntent({ type: "activate", action: "choose-destination" });
  };
  const useDestinationDraft = () => {
    const destination = destinationDraftRef.current?.value ?? "";
    if (!destination) return;
    submittedDestinationRef.current = destination;
    chooserRevisionAtStartRef.current = null;
    onIntent({ type: "set-field", field: "destination", value: destination });
  };
  useEffect(() => {
    if (!destinationDraftWasAccepted(
      submittedDestinationRef.current,
      chooserRevisionAtStartRef.current,
      model,
    )) return;
    submittedDestinationRef.current = null;
    chooserRevisionAtStartRef.current = null;
    clearDestinationDraft();
  }, [generation, model.chooserRevision, model.destination]);

  return (
    <div data-mesh-proof="workspace-destination" data-mesh-generation={generation} aria-label="Update destination" className="grid gap-5 p-5 lg:p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="max-w-3xl">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Update destination</p>
            <Badge tone="warning">Explicit and staged</Badge>
          </div>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">Update saved work in an ordinary folder</h2>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">Mesh remembers and prefills the original folder used for an import, including after you switch to an older saved version. For a workspace opened manually, choose a destination once. Choose one saved file or the complete saved workspace; Mesh verifies the destination and shows the exact create, replace, or removal plan before a separate confirmation can write anything. Every file is rechecked and installed atomically.</p>
        </div>
      </header>

      <div className="grid gap-4 lg:grid-cols-2">
        <label className="grid gap-2 text-sm font-semibold" htmlFor="destination-next-file">
          Saved file
          <select ref={selectedFileRef} id="destination-next-file" value={model.selectedFile} disabled={!model.canSelectFile} className="min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" onChange={(event) => onIntent({ type: "set-field", field: "selectedFile", value: event.currentTarget.value })}>
            <option value="" disabled={model.files.length > 0}>{model.files.length ? "Choose a saved file" : "No saved files available"}</option>
            {model.files.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}
          </select>
        </label>
        <div className="grid gap-2">
          <p className="text-sm font-semibold">Selected destination</p>
          <output data-mesh-proof="destination-selected" aria-label={model.destination ? `Selected destination: ${model.destination}` : "No destination selected"} aria-live="polite" tabIndex={0} className="min-h-11 min-w-0 select-text overflow-x-auto rounded-lg border border-border bg-muted/40 px-3 py-2.5 font-mono text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring">{model.destination}</output>
          {!model.destination ? <p className="text-xs text-muted-foreground">No destination selected.</p> : null}
          <div className="flex flex-col gap-2 sm:flex-row">
            <Button data-mesh-proof="destination-choose" variant="secondary" disabled={!action("choose-destination").enabled} onClick={chooseDestination}>{action("choose-destination").label}</Button>
          </div>
        </div>
      </div>

      <div className="grid gap-2 rounded-xl border border-border p-4">
        <label className="text-sm font-semibold" htmlFor="destination-next-draft">Or type a destination folder</label>
        <p id="destination-next-draft-hint" className="text-xs leading-5 text-muted-foreground">Type an absolute folder path, then choose Use typed destination or press Enter. The selected destination above is the value Mesh will preview.</p>
        <div className="flex flex-col gap-2 sm:flex-row">
          <input ref={destinationDraftRef} data-mesh-proof="destination-draft" id="destination-next-draft" defaultValue="" disabled={!model.canEditDestination} aria-describedby="destination-next-draft-hint" placeholder="/absolute/path/private-copy" className="min-h-11 min-w-0 flex-1 rounded-lg border border-border bg-background px-3 text-sm font-normal text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" onChange={(event) => setDraftPresent(event.currentTarget.value.length > 0)} onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            useDestinationDraft();
          }} />
          <Button variant="secondary" disabled={!model.canEditDestination || !draftPresent} onClick={useDestinationDraft}>Use typed destination</Button>
        </div>
      </div>

      <p data-mesh-proof="destination-hint" id="destination-next-hint" className="text-sm leading-6 text-muted-foreground" aria-live="polite">{model.hint}</p>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="grid gap-2 rounded-xl border border-border p-4">
          <p className="text-sm font-semibold">One saved file</p>
          <p className="text-xs leading-5 text-muted-foreground">Preview the selected file against the destination, then create or replace it only if the exact preview remains current.</p>
          <div className="mt-auto grid gap-2">{button("preview-single", "secondary")}{button("confirm-single", "primary")}</div>
        </div>
        <div className="grid gap-2 rounded-xl border border-border p-4">
          <p className="text-sm font-semibold">Saved workspace</p>
          <p className="text-xs leading-5 text-muted-foreground">Preview folders and files together. Mesh creates reviewed missing folders first, then automatically re-previews the files and updates only the verified changed set after confirmation.</p>
          <div className="mt-auto grid gap-2">{button("preview-all", "secondary", "destination-preview-all")}{button("confirm-batch", "primary", "destination-confirm-all")}</div>
        </div>
      </div>

      {model.plan ? <DestinationPlan plan={model.plan} /> : (
        <div className="rounded-xl border border-dashed border-border p-5 text-sm leading-6 text-muted-foreground">Choose a destination and preview one file or the saved workspace. No destination entry changes until you review a plan and confirm it.</div>
      )}

      <p className="border-t border-border pt-4 text-xs leading-5 text-muted-foreground">Mesh creates folders and updates current files first. After current paths are installed, Mesh derives former saved paths for a separate reviewed removal step. Only paths proven to come from the original import or an exact durable update receipt are eligible; recursive deletion is unavailable. Private-only, changed, replaced, or unrelated destination entries are preserved, and Mesh never removes a destination entry silently. A batch never rolls back an earlier completed prefix.</p>
    </div>
  );
}

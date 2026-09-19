import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import type { ImportReviewFocusAuthorization } from "../models/import-focus";
import type { ImportWorkbenchIntent, ImportWorkbenchModel } from "../models/import-workbench";

export function ImportWorkbench({ model, onIntent, reviewFocusAuthorization = null }: {
  model: ImportWorkbenchModel;
  onIntent: (intent: ImportWorkbenchIntent) => void;
  reviewFocusAuthorization?: ImportReviewFocusAuthorization | null;
}) {
  const [path, setPath] = useState(model.sourcePath);
  const [destination, setDestination] = useState(model.destinationPath);
  const reviewHeading = useRef<HTMLHeadingElement>(null);
  useEffect(() => setPath(model.sourcePath), [model.sourcePath]);
  useEffect(() => setDestination(model.destinationPath), [model.destinationPath]);
  useLayoutEffect(() => {
    if (model.phase === "review" && reviewFocusAuthorization?.consume()) {
      reviewHeading.current?.focus({ preventScroll: true });
    }
    return () => reviewFocusAuthorization?.cancel();
  }, [reviewFocusAuthorization, model.phase]);
  if (model.phase === "review") {
    return (
      <section
        className="grid gap-5"
        aria-label="Verified folder preview"
        aria-busy={model.busy}
        data-mesh-proof="import-verified-preview"
      >
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Verified copy</p>
            <Badge tone={model.busy ? "neutral" : "positive"}>
              {model.busy ? "Creating private workspace" : "Ready to create"}
            </Badge>
          </div>
          <h3
            ref={reviewHeading}
            tabIndex={-1}
            data-mesh-proof="import-review-heading"
            className="mt-2 text-xl font-semibold focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            Review what Mesh will bring in
          </h3>
          <p className="mt-2 break-all text-sm text-muted-foreground">{model.sourcePath}</p>
        </div>
        <dl className="grid grid-cols-3 gap-2 text-sm">
          <Fact label="Files" value={model.fileCount} />
          <Fact label="Folders" value={model.folderCount} />
          <Fact label="Size" value={model.byteCount} />
        </dl>
        <div className="rounded-lg border border-border bg-muted/20 p-4">
          <p className="text-sm leading-6 text-muted-foreground">{model.scopeNote}</p>
          {model.busy ? (
            <p
              className="mt-3 text-sm font-medium"
              role="status"
              aria-live="polite"
              data-mesh-proof="import-confirmation-progress"
            >
              Keep Mesh open. Large projects can take several minutes; the original folder remains unchanged.
            </p>
          ) : null}
          {model.files.length > 0 ? (
            <details className="mt-3">
              <summary className="min-h-11 cursor-pointer text-sm font-medium">Review included files</summary>
              <ul className="mt-2 max-h-56 overflow-auto rounded-md border border-border bg-background/50 p-3 font-mono text-xs leading-6">
                {model.files.map((line) => <li key={line} className="break-all">{line}</li>)}
              </ul>
            </details>
          ) : null}
        </div>
        <div className="rounded-lg border border-border bg-background/40 p-3">
          <p className="text-xs text-muted-foreground">Verified summary</p>
          <p className="mt-1 break-all font-mono text-xs">{model.summary}</p>
        </div>
        <details className="rounded-lg border border-border bg-background/40 p-3">
          <summary className="min-h-11 cursor-pointer text-sm font-medium">Use a custom private location</summary>
          <label className="mt-3 grid gap-2 text-sm font-medium">
            Custom location
            <div className="flex flex-col gap-2 sm:flex-row">
              <input
                data-mesh-proof="import-destination"
                className="min-h-11 min-w-0 flex-1 rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                value={destination}
                disabled={!model.canEditDestination}
                onChange={(event) => {
                  const draft = event.target.value;
                  setDestination(draft);
                  onIntent({ type: "destination-draft", path: draft });
                }}
                placeholder="Leave blank for Mesh-managed storage"
                autoComplete="off"
              />
              <Button
                variant="secondary"
                disabled={!model.canChooseDestination}
                onClick={() => onIntent({ type: "choose-destination" })}
              >
                Choose parent
              </Button>
            </div>
          </label>
        </details>
        <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
          <Button
            variant="secondary"
            disabled={!model.canChoose}
            onClick={() => onIntent({ type: "choose-folder" })}
          >
            Choose another folder
          </Button>
          <Button variant="primary" disabled={!model.canConfirm} onClick={() => onIntent({ type: "confirm-import" })}>{model.confirmLabel}</Button>
        </div>
        <p className="text-center text-xs text-muted-foreground">The original folder is never opened for writing.</p>
      </section>
    );
  }
  const previewTypedPath = () => {
    const candidate = path;
    if (candidate) onIntent({ type: "preview-path", path: candidate });
  };
  return (
    <section className="grid gap-5" aria-label="Bring in a folder" data-mesh-proof="import-select">
      <div>
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Start with your folder</p>
        <h3 className="mt-2 text-2xl font-semibold tracking-tight">Keep working in ordinary files</h3>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">Mesh verifies the folder before creating a private workspace. Your original stays untouched.</p>
      </div>
      <Button data-mesh-import-choose variant="primary" disabled={!model.canChoose} onClick={() => onIntent({ type: "choose-folder" })}>Choose a folder</Button>
      <div className="flex items-center gap-3" aria-hidden="true"><span className="h-px flex-1 bg-border" /><span className="text-xs text-muted-foreground">or use a path</span><span className="h-px flex-1 bg-border" /></div>
      <label className="grid gap-2 text-sm font-medium">
        Absolute folder path
        <div className="flex flex-col gap-2 sm:flex-row">
          <input
            data-mesh-proof="import-path"
            className="min-h-11 min-w-0 flex-1 rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            value={path}
            onChange={(event) => {
              const draft = event.target.value;
              setPath(draft);
              onIntent({ type: "source-draft", path: draft });
            }}
            onKeyDown={(event) => { if (event.key === "Enter") previewTypedPath(); }}
            placeholder="/Users/you/Project"
            autoComplete="off"
          />
          <Button data-mesh-proof="import-preview" variant="secondary" disabled={!model.canPreviewPath || path.length === 0} onClick={previewTypedPath}>Preview folder</Button>
        </div>
      </label>
    </section>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-background/40 p-3">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-1 font-semibold">{value}</dd>
    </div>
  );
}

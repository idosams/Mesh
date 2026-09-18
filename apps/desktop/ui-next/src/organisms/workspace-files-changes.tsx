import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MutableRefObject,
} from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { SegmentedControl } from "../atoms/segmented-control";
import type {
  WorkspaceFilesChangesIntent,
  WorkspaceFilesChangesModel,
  WorkspaceNativeChange,
  WorkspaceWorkActionId,
} from "../models/workspace-files-changes";
import {
  workspaceChoiceProjection,
  workspaceChangeFocusNavigation,
  workspaceChangeProjection,
  workspaceChangeNavigation,
  workspaceChangeRovingPath,
  workspaceExplorerAncestorPaths,
  workspaceFilePresentation,
  workspaceExplorerLocateFilter,
  workspaceExplorerNavigation,
  workspaceExplorerProjection,
  workspaceExplorerSummary,
  workspaceExplorerTree,
  workspaceTextPreview,
  type WorkspaceExplorerNode,
  type WorkspaceExplorerRow,
  type WorkspaceChangeFilter,
} from "../models/workspace-explorer";
import {
  workspaceSplitDiffRows,
  workspaceTextDiff,
  type WorkspaceTextDiffHunk,
  type WorkspaceTextDiffLine,
} from "../models/workspace-text-diff";

type WorkbenchProps = {
  model: WorkspaceFilesChangesModel;
  onIntent: (intent: WorkspaceFilesChangesIntent) => void;
};

function actionMap(model: WorkspaceFilesChangesModel) {
  return new Map(model.actions.map((action) => [action.id, action]));
}

function Chevron({ direction = "right" }: { direction?: "right" | "down" }) {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className={`size-3.5 fill-none stroke-current stroke-[1.75] ${direction === "down" ? "rotate-90" : ""}`}>
      <path d="m6 3.5 4.5 4.5L6 12.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function SearchGlyph() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="size-4 fill-none stroke-current stroke-[1.5]">
      <circle cx="7" cy="7" r="4.25" /><path d="m10.25 10.25 3 3" strokeLinecap="round" />
    </svg>
  );
}

function RepositoryGlyph() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="size-4 fill-none stroke-current stroke-[1.4]">
      <path d="M3 2.5h8.5A1.5 1.5 0 0 1 13 4v9.5H4.5A1.5 1.5 0 0 1 3 12V2.5Z" /><path d="M5.5 2.5v11M3 11.5h8" />
    </svg>
  );
}

function FileGlyph({ node }: { node: Pick<WorkspaceExplorerNode, "kind" | "path"> }) {
  if (node.kind === "folder") {
    return (
      <svg viewBox="0 0 16 16" aria-hidden="true" className="size-4 fill-sky-300/20 stroke-sky-300 stroke-[1.25]">
        <path d="M1.75 4.25h4l1.25 1.5h7.25v6.5A1.25 1.25 0 0 1 13 13.5H3a1.25 1.25 0 0 1-1.25-1.25v-8Z" strokeLinejoin="round" />
      </svg>
    );
  }
  const category = workspaceFilePresentation(node.path).category;
  const tone = {
    code: "text-sky-300",
    document: "text-violet-300",
    image: "text-fuchsia-300",
    config: "text-amber-300",
    data: "text-emerald-300",
    system: "text-muted-foreground",
    file: "text-muted-foreground",
  }[category];
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className={`size-4 fill-none stroke-current stroke-[1.25] ${tone}`}>
      <path d="M3 1.75h6l4 4v8.5H3z" strokeLinejoin="round" /><path d="M9 1.75v4h4" strokeLinejoin="round" />
      {category === "code" ? <path d="m6.5 8-1.5 1.5L6.5 11m3-3L11 9.5 9.5 11" strokeLinecap="round" strokeLinejoin="round" /> : null}
    </svg>
  );
}

function WorkspaceTree({ rows, selectedEntry, focusedPath, disabled, changes, rowRefs, onSelect, onToggle, onKeyDown }: {
  rows: readonly WorkspaceExplorerRow[];
  selectedEntry: string;
  focusedPath: string;
  disabled: boolean;
  changes: ReadonlyMap<string, WorkspaceNativeChange>;
  rowRefs: MutableRefObject<Map<string, HTMLButtonElement>>;
  onSelect: (path: string) => void;
  onToggle: (path: string) => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLButtonElement>, path: string) => void;
}) {
  return (
    <div role="tree" aria-label="Workspace files" className="grid gap-px" data-mesh-proof="files-tree">
      {rows.map((row) => {
        const change = changes.get(row.node.path);
        const presentation = row.node.kind === "file" ? workspaceFilePresentation(row.node.path) : null;
        return (
        <button
          key={row.node.path}
          ref={(element) => {
            if (element) rowRefs.current.set(row.node.path, element);
            else rowRefs.current.delete(row.node.path);
          }}
          type="button"
          role="treeitem"
          aria-level={row.level}
          aria-expanded={row.node.kind === "folder" && row.node.children.length > 0 ? row.expanded : undefined}
          aria-selected={selectedEntry === row.node.path}
          tabIndex={focusedPath === row.node.path ? 0 : -1}
          className={`group relative flex min-h-11 w-full items-center gap-2 rounded-sm py-2 pr-2 text-left font-mono text-[13px] hover:bg-muted/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring disabled:opacity-50 ${selectedEntry === row.node.path ? "bg-sky-300/10 text-foreground before:absolute before:inset-y-1 before:left-0 before:w-0.5 before:bg-sky-300" : "text-muted-foreground"}`}
          style={{ paddingLeft: `${8 + (row.level - 1) * 20}px` }}
          disabled={disabled}
          onClick={() => {
            onSelect(row.node.path);
            if (row.node.kind === "folder" && row.node.children.length > 0) onToggle(row.node.path);
          }}
          onKeyDown={(event) => onKeyDown(event, row.node.path)}
          data-mesh-work-entry={row.node.path}
        >
          <span aria-hidden="true" className="grid w-4 shrink-0 place-items-center text-muted-foreground">
            {row.node.kind === "folder" && row.node.children.length > 0 ? <Chevron direction={row.expanded ? "down" : "right"} /> : null}
          </span>
          <span className="grid w-5 shrink-0 place-items-center"><FileGlyph node={row.node} /></span>
          <span className="min-w-0 flex-1 truncate">{row.node.name}</span>
          {presentation ? <span className="max-w-14 shrink-0 truncate font-sans text-[9px] font-semibold uppercase tracking-wide text-muted-foreground" title={presentation.label} data-mesh-file-type={presentation.shortLabel}>{presentation.shortLabel}</span> : null}
          {change ? (
            <span
              className={`grid size-5 shrink-0 place-items-center rounded-sm font-sans text-[10px] font-bold ${change.code === "A" ? "text-emerald-300" : change.code === "D" ? "text-red-300" : change.code === "M" ? "text-amber-300" : "text-muted-foreground"}`}
              aria-label={change.status}
              title={change.status}
              data-mesh-explorer-change={change.code}
            >{change.code}</span>
          ) : null}
          <span className="sr-only">{row.node.kind}</span>
        </button>
        );
      })}
    </div>
  );
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

type SelectedWorkspaceEntry = WorkspaceFilesChangesModel["files"]["entries"][number];

function ReadOnlyTextPreview({ path, text }: { path: string; text: string }) {
  const preview = useMemo(() => workspaceTextPreview(text), [text]);
  return (
    <section className="min-w-0 overflow-hidden rounded-md border border-border bg-[#070b10]" aria-label={`Current content of ${path}`}>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-muted/25 px-3 py-2">
        <h4 className="text-xs font-semibold">Current file</h4>
        <span className="font-mono text-[11px] text-muted-foreground">{preview.totalLines.toLocaleString()} {preview.totalLines === 1 ? "line" : "lines"}</span>
      </div>
      <div className="max-h-[34rem] overflow-auto font-mono text-[13px] leading-6" tabIndex={0}>
        {preview.lines.map((line, index) => (
          <div className="grid min-h-6 grid-cols-[3.5rem_minmax(0,1fr)]" key={`${index}:${line}`}>
            <span aria-hidden="true" className="select-none border-r border-border/60 px-2 text-right text-muted-foreground/70">{index + 1}</span>
            <code className="whitespace-pre-wrap break-words px-3">{line || " "}</code>
          </div>
        ))}
      </div>
      {preview.truncated ? <p className="border-t border-border bg-amber-400/5 px-3 py-2 text-xs text-amber-100" role="status">Previewing the first {preview.lines.length.toLocaleString()} of {preview.totalLines.toLocaleString()} lines. Open the exact file for the complete content.</p> : null}
    </section>
  );
}

function WorkspaceFilePreview({ selected, selectedChange, changes }: {
  selected: SelectedWorkspaceEntry;
  selectedChange: WorkspaceNativeChange | null;
  changes: WorkspaceFilesChangesModel["changes"];
}) {
  if (selected.kind === "folder") {
    return <div className="grid min-h-64 place-items-center rounded-md border border-dashed border-border p-8 text-center"><div><h4 className="font-semibold">Folder selected</h4><p className="mt-2 text-sm text-muted-foreground">Use Open in Finder to browse this exact folder, or expand it in Explorer.</p></div></div>;
  }
  const matchesInspection = changes.selectedFile === selected.value && changes.editorKind !== "none";
  if (!matchesInspection) {
    const canInspect = changes.canSelectFile && changes.files.some((choice) => choice.value === selected.value);
    return <div className="grid min-h-64 place-items-center rounded-md border border-dashed border-border p-8 text-center" aria-live="polite"><div className="max-w-lg"><h4 className="font-semibold">{canInspect ? "Loading exact file preview…" : "Inline preview unavailable"}</h4><p className="mt-2 text-sm leading-6 text-muted-foreground">{canInspect ? "Mesh is verifying the current working bytes before showing content." : "Mesh cannot safely inspect this file in the current workspace state. The external Open and Finder actions remain available."}</p></div></div>;
  }
  if (changes.editorKind === "binary") {
    return <div className="grid min-h-64 place-items-center rounded-md border border-dashed border-border p-8 text-center"><div className="max-w-lg"><h4 className="font-semibold">Native preview required</h4><p className="mt-2 text-sm leading-6 text-muted-foreground">{workspaceFilePresentation(selected.value).label} content is kept outside the text renderer. Open the exact working file with its default app.</p></div></div>;
  }
  if (selectedChange && changes.baselineAvailable) {
    return <TextComparison before={changes.baselineText} after={changes.editorText} split={false} baselineAvailable />;
  }
  return <ReadOnlyTextPreview path={selected.value} text={changes.editorText} />;
}

function WorkspaceFileDetails({ selected, selectedChange }: {
  selected: SelectedWorkspaceEntry;
  selectedChange: WorkspaceNativeChange | null;
}) {
  const presentation = selected.kind === "file" ? workspaceFilePresentation(selected.value) : null;
  const folder = selected.value.includes("/") ? selected.value.slice(0, selected.value.lastIndexOf("/")) : "Workspace root";
  return <section className="overflow-hidden rounded-md border border-border bg-background/35" aria-label="File details">
    <div className="flex items-center justify-between border-b border-border bg-muted/30 px-4 py-3">
      <div><h4 className="text-sm font-semibold">Details</h4><p className="mt-0.5 text-xs text-muted-foreground">Identity and working-tree state</p></div>
      <Badge tone="neutral">{presentation?.shortLabel ?? "FOLDER"}</Badge>
    </div>
    <dl className="grid text-sm sm:grid-cols-[9rem_minmax(0,1fr)]">
      {selectedChange ? <><dt className="border-b border-border px-4 py-3 font-medium text-muted-foreground sm:border-r">Working tree</dt><dd className="border-b border-border px-4 py-3"><span className="font-semibold">{selectedChange.status}</span><span className="ml-2 text-muted-foreground">{selectedChange.detail || selectedChange.description}</span></dd></> : null}
      <dt className="border-b border-border px-4 py-3 font-medium text-muted-foreground sm:border-r">Path</dt><dd className="break-all border-b border-border px-4 py-3 font-mono">{selected.value}</dd>
      <dt className="border-b border-border px-4 py-3 font-medium text-muted-foreground sm:border-r">Parent</dt><dd className="break-all border-b border-border px-4 py-3 font-mono">{folder}</dd>
      <dt className="px-4 py-3 font-medium text-muted-foreground sm:border-r">Type</dt><dd className="px-4 py-3">{selected.kind === "file" ? presentation?.label : "Folder"}</dd>
    </dl>
  </section>;
}

export function WorkspaceFiles({ model, onIntent }: WorkbenchProps) {
  const actions = useMemo(() => actionMap(model), [model]);
  const tree = useMemo(() => workspaceExplorerTree(model.files.entries), [model.files.entries]);
  const [filter, setFilter] = useState("");
  const [explorerOpen, setExplorerOpen] = useState(true);
  const [fileView, setFileView] = useState<"Preview" | "Details">("Preview");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(
    workspaceExplorerAncestorPaths(model.files.selectedEntry),
  ));
  const [focusedPath, setFocusedPath] = useState(model.files.selectedEntry || tree[0]?.path || "");
  const explorerProjection = useMemo(
    () => workspaceExplorerProjection(tree, expanded, filter),
    [tree, expanded, filter],
  );
  const rows = explorerProjection.rows;
  const rowRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const newPathRef = useRef<HTMLInputElement>(null);
  const movePathRef = useRef<HTMLInputElement>(null);
  const selected = model.files.entries.find((entry) => entry.value === model.files.selectedEntry) ?? null;
  const changesByPath = useMemo(
    () => new Map(model.changes.queue.map((change) => [change.path, change] as const)),
    [model.changes.queue],
  );
  const selectedChange = selected ? changesByPath.get(selected.value) ?? null : null;
  const summary = useMemo(() => workspaceExplorerSummary(model.files.entries), [model.files.entries]);
  const breadcrumbs = model.files.selectedEntry.split("/").filter(Boolean).map((name, index, parts) => ({
    name,
    path: parts.slice(0, index + 1).join("/"),
  }));
  const selectedName = selected?.value.split("/").at(-1) ?? "No file selected";
  const selectedPresentation = selected?.kind === "file" ? workspaceFilePresentation(selected.value) : null;

  useEffect(() => {
    if (rows.some((row) => row.node.path === focusedPath)) return;
    setFocusedPath(model.files.selectedEntry && rows.some((row) => row.node.path === model.files.selectedEntry)
      ? model.files.selectedEntry
      : rows[0]?.node.path ?? "");
  }, [focusedPath, model.files.selectedEntry, rows]);

  const toggle = (path: string) => setExpanded((current) => {
    const next = new Set(current);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    return next;
  });
  const focusRow = (path: string) => {
    setFocusedPath(path);
    window.requestAnimationFrame(() => rowRefs.current.get(path)?.focus());
  };
  const handleTreeKey = (event: ReactKeyboardEvent<HTMLButtonElement>, path: string) => {
    if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const navigation = workspaceExplorerNavigation(
      rows,
      path,
      event.key as "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Home" | "End",
    );
    if (!navigation) return;
    event.preventDefault();
    if (navigation.expandPath) setExpanded((current) => new Set([...current, navigation.expandPath as string]));
    if (navigation.collapsePath) setExpanded((current) => {
      const next = new Set(current);
      next.delete(navigation.collapsePath as string);
      return next;
    });
    focusRow(navigation.focusPath);
  };

  return (
    <div className="grid" aria-label="Workspace file explorer" data-mesh-proof="files-explorer">
      <header className="flex min-h-16 flex-wrap items-center justify-between gap-3 border-b border-border bg-background/35 px-4 py-2">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-md border border-border bg-muted text-sky-300"><RepositoryGlyph /></span>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="truncate text-base font-semibold tracking-tight">{model.files.workspaceLabel}</h2>
              <Badge tone={model.files.workspaceState === "agent-assigned" ? "warning" : "neutral"}>
                {model.files.workspaceState === "agent-assigned" ? "Agent assigned" : "Current workspace"}
              </Badge>
            </div>
            <p className="mt-0.5 truncate font-mono text-xs text-muted-foreground" title={model.files.workspaceRoot}>{model.files.workspaceRoot}</p>
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center gap-3">
          <p className="text-xs text-muted-foreground"><strong className="text-foreground">{summary.files}</strong> files · <strong className="text-foreground">{summary.folders}</strong> folders · <strong className={model.changes.queue.length ? "text-amber-300" : "text-foreground"}>{model.changes.queue.length}</strong> changes</p>
          <WorkAction id="open-workspace-folder" actions={actions} onIntent={onIntent} />
        </div>
      </header>

      <div className={`grid min-h-[42rem] overflow-hidden bg-card ${explorerOpen ? "lg:grid-cols-[20rem_minmax(0,1fr)]" : "lg:grid-cols-[3.25rem_minmax(0,1fr)]"}`}>
        {explorerOpen ? <section id="workspace-files-explorer-panel" className="min-w-0 border-b border-border bg-background/55 lg:border-b-0 lg:border-r" aria-labelledby="workspace-tree-heading">
          <div className="flex min-h-12 items-center justify-between gap-2 border-b border-border px-3">
            <div>
              <h3 id="workspace-tree-heading" className="text-xs font-semibold uppercase tracking-[0.14em]">Explorer</h3>
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                {explorerProjection.truncated
                  ? `${rows.length} of ${explorerProjection.matched} visible paths shown`
                  : `${rows.length} of ${summary.total} visible`}
              </p>
            </div>
            <div className="flex items-center gap-1">
              <Button className="px-2" size="compact" variant="quiet" aria-controls="workspace-files-explorer-panel" aria-expanded="true" onClick={() => setExplorerOpen(false)}>Hide</Button>
              <Button className="px-2" size="compact" variant="quiet" onClick={() => setExpanded(new Set())}>Collapse</Button>
              <Button
                className="px-2"
                size="compact"
                variant="quiet"
                disabled={!model.files.selectedEntry}
                onClick={() => {
                  setFilter(workspaceExplorerLocateFilter(tree, model.files.selectedEntry));
                  setExpanded(new Set(workspaceExplorerAncestorPaths(model.files.selectedEntry)));
                  focusRow(model.files.selectedEntry);
                }}
              >
                Locate
              </Button>
            </div>
          </div>
          <label className="relative m-3 block">
            <span className="sr-only">Filter workspace files</span>
            <span className="pointer-events-none absolute inset-y-0 left-3 grid place-items-center text-muted-foreground"><SearchGlyph /></span>
            <input
              data-mesh-proof="files-filter"
              type="search"
              value={filter}
              onChange={(event) => setFilter(event.currentTarget.value)}
              placeholder="Go to file…"
              className="min-h-11 w-full rounded-md border border-border bg-background py-2 pl-9 pr-3 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
            />
          </label>
          <div className="max-h-[34rem] overflow-auto px-2 pb-3">
            {rows.length ? (
              <>
                <WorkspaceTree
                  rows={rows}
                  selectedEntry={model.files.selectedEntry}
                  focusedPath={focusedPath || rows[0].node.path}
                  disabled={!model.files.canSelectEntry}
                  changes={changesByPath}
                  rowRefs={rowRefs}
                  onSelect={(value) => {
                    setFocusedPath(value);
                    setExpanded((current) => new Set([
                      ...current,
                      ...workspaceExplorerAncestorPaths(value),
                    ]));
                    onIntent({ type: "set-field", field: "selectedEntry", value });
                  }}
                  onToggle={toggle}
                  onKeyDown={handleTreeKey}
                />
                {explorerProjection.truncated ? (
                  <p className="m-2 rounded-md border border-amber-400/25 bg-amber-400/5 p-3 text-xs leading-5 text-amber-100" role="status">
                    Showing the first {rows.length} visible paths. Collapse folders or refine the file search to narrow the result set.
                  </p>
                ) : null}
              </>
            ) : (
              <p className="rounded-md border border-dashed border-border p-4 text-sm text-muted-foreground" role="status">
                {model.files.entries.length ? "No files match this filter." : "This workspace has no files or folders yet."}
              </p>
            )}
          </div>
        </section> : <aside id="workspace-files-explorer-panel" className="flex min-h-12 items-start justify-center border-b border-border bg-background/55 p-2 lg:border-b-0 lg:border-r" aria-label="Explorer collapsed">
          <Button className="px-2 lg:[writing-mode:vertical-rl]" size="compact" variant="quiet" aria-controls="workspace-files-explorer-panel" aria-expanded="false" onClick={() => setExplorerOpen(true)}>Show Explorer</Button>
        </aside>}

        <section className="grid min-w-0 grid-rows-[auto_auto_1fr_auto] bg-[#0b1016]" aria-labelledby="workspace-details-heading">
          <nav aria-label="Selected path" className="flex min-h-12 min-w-0 items-center gap-1 overflow-x-auto border-b border-border bg-background/30 px-4 text-xs text-muted-foreground">
            <span className="flex shrink-0 items-center gap-1.5 font-semibold text-foreground"><RepositoryGlyph />{model.files.workspaceLabel}</span>
            {breadcrumbs.map((part) => (
              <span key={part.path} className="flex shrink-0 items-center gap-1">
                <Chevron />
                <span className="font-mono">{part.name}</span>
              </span>
            ))}
          </nav>

          <div className="flex flex-wrap items-center justify-between gap-4 border-b border-border bg-card/60 px-4 py-3">
            <div className="flex min-w-0 items-start gap-3">
              <span className="grid size-8 shrink-0 place-items-center rounded-md border border-border bg-muted"><FileGlyph node={{ kind: selected?.kind ?? "file", path: selected?.value ?? "" }} /></span>
              <div className="min-w-0" data-mesh-proof="files-selected-entry" data-mesh-entry-path={selected?.value ?? ""}>
                <div className="flex flex-wrap items-center gap-2">
                  <h3 id="workspace-details-heading" className="break-all font-mono text-sm font-semibold">{selectedName}</h3>
                  {selectedPresentation ? <Badge tone="neutral">{selectedPresentation.label}</Badge> : null}
                  {selectedChange ? <Badge tone="changed">{selectedChange.status}</Badge> : null}
                </div>
                <p className="mt-0.5 text-xs text-muted-foreground">{selected ? `${selected.kind === "file" ? "Regular file" : "Directory"} · exact mounted workspace` : "Select an item from Files"}</p>
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              {selected ? <SegmentedControl label="File information view" value={fileView} onChange={(value) => setFileView(value as "Preview" | "Details")} options={["Preview", "Details"]} /> : null}
              {selected ? <WorkAction id="open-entry" actions={actions} onIntent={onIntent} variant="primary" /> : null}
              {selected?.kind === "file" ? <WorkAction id="reveal-entry" actions={actions} onIntent={onIntent} /> : null}
            </div>
          </div>

          <div className="min-w-0 p-4">
            {selected ? (
              fileView === "Preview"
                ? <WorkspaceFilePreview selected={selected} selectedChange={selectedChange} changes={model.changes} />
                : <WorkspaceFileDetails selected={selected} selectedChange={selectedChange} />
            ) : (
              <div className="grid min-h-52 place-items-center rounded-lg border border-dashed border-border p-8 text-center">
                <div><p className="font-semibold">Choose a file or folder</p><p className="mt-2 text-sm text-muted-foreground">Use Files to inspect an exact workspace entry.</p></div>
              </div>
            )}
          </div>

          <details className="border-t border-border bg-background/35">
            <summary className="flex min-h-12 cursor-pointer items-center gap-2 px-5 text-sm font-semibold hover:bg-muted/50"><Chevron /> Repository actions</summary>
            <div className="grid gap-4 border-t border-border p-5 xl:grid-cols-3">
              <section aria-labelledby="new-entry-heading">
                <h4 id="new-entry-heading" className="text-sm font-semibold">New</h4>
                <label className="mt-2 grid gap-2 text-xs font-medium text-muted-foreground">Relative path
                  <input ref={newPathRef} className="min-h-11 rounded-md border border-border bg-background px-3 font-mono text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" value={model.files.newPath} disabled={!model.files.canEditNewPath} placeholder="notes/idea.txt" onChange={(event) => onIntent({ type: "set-field", field: "newPath", value: event.currentTarget.value })} />
                </label>
                <div className="mt-3 flex flex-wrap gap-2"><WorkAction id="create-text" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "newPath", value: newPathRef.current?.value ?? model.files.newPath })} /><WorkAction id="create-folder" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "newPath", value: newPathRef.current?.value ?? model.files.newPath })} /></div>
              </section>
              <section className="border-t border-border pt-4 xl:border-l xl:border-t-0 xl:pl-4 xl:pt-0" aria-labelledby="move-entry-heading">
                <h4 id="move-entry-heading" className="text-sm font-semibold">Rename or move</h4>
                <label className="mt-2 grid gap-2 text-xs font-medium text-muted-foreground">New relative path
                  <input ref={movePathRef} className="min-h-11 rounded-md border border-border bg-background px-3 font-mono text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" value={model.files.movePath} disabled={!model.files.canEditMovePath} placeholder="notes/final.txt" onChange={(event) => onIntent({ type: "set-field", field: "movePath", value: event.currentTarget.value })} />
                </label>
                <div className="mt-3"><WorkAction id="move-entry" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "movePath", value: movePathRef.current?.value ?? model.files.movePath })} /></div>
              </section>
              <section className="border-t border-red-400/25 pt-4 xl:border-l xl:border-t-0 xl:pl-4 xl:pt-0" aria-labelledby="delete-entry-heading">
                <h4 id="delete-entry-heading" className="text-sm font-semibold text-red-200">Delete selected item</h4>
                <p className="mt-2 text-xs leading-5 text-muted-foreground">Deletion accepts files and empty folders and requires confirmation. Saved file content remains in immutable history.</p>
                <div className="mt-3"><WorkAction id="delete-entry" actions={actions} onIntent={onIntent} variant="danger" /></div>
              </section>
            </div>
          </details>
        </section>
      </div>
      <p className="flex min-h-9 items-center border-t border-border bg-[#0b1016] px-3 text-xs text-muted-foreground" role="status" aria-live="polite">{model.files.status}</p>
    </div>
  );
}

export function WorkspaceChanges({ model, onIntent }: WorkbenchProps) {
  const actions = useMemo(() => actionMap(model), [model]);
  const [changesPanelOpen, setChangesPanelOpen] = useState(true);
  const [view, setView] = useState<"Edit" | "Inline" | "Split">("Inline");
  const [changeQuery, setChangeQuery] = useState("");
  const [changeFilter, setChangeFilter] = useState<WorkspaceChangeFilter>("all");
  const [missingSourceQuery, setMissingSourceQuery] = useState("");
  const [moveTargetQuery, setMoveTargetQuery] = useState("");
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const changeButtonRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const missingSourceRef = useRef<HTMLSelectElement>(null);
  const moveTargetRef = useRef<HTMLSelectElement>(null);
  const changeRows = model.changes.queue;
  const editablePaths = useMemo(() => new Set(model.changes.files.map((file) => file.value)), [model.changes.files]);
  const loadFile = actions.get("load-file");
  const changeProjection = useMemo(
    () => workspaceChangeProjection(
      changeRows,
      changeQuery,
      changeFilter,
      undefined,
      model.changes.selectedFile,
    ),
    [changeFilter, changeQuery, changeRows, model.changes.selectedFile],
  );
  const changeGroups = changeProjection.groups;
  const changeCounts = useMemo(() => ({
    all: changeRows.length,
    M: changeRows.filter((change) => change.code === "M").length,
    A: changeRows.filter((change) => change.code === "A").length,
    D: changeRows.filter((change) => change.code === "D").length,
    "?": changeRows.filter((change) => change.code === "?").length,
  }), [changeRows]);
  const inspectablePaths = useMemo(() => changeProjection.orderedPaths
    .filter((path) => editablePaths.has(path) && Boolean(loadFile?.enabled) && model.changes.canSelectFile), [changeProjection.orderedPaths, editablePaths, loadFile?.enabled, model.changes.canSelectFile]);
  const visibleInspectablePaths = useMemo(() => changeGroups
    .flatMap((group) => group.changes.map((change) => change.path))
    .filter((path) => editablePaths.has(path) && Boolean(loadFile?.enabled) && model.changes.canSelectFile), [changeGroups, editablePaths, loadFile?.enabled, model.changes.canSelectFile]);
  const [focusedChangePath, setFocusedChangePath] = useState(
    model.changes.selectedFile || visibleInspectablePaths[0] || "",
  );
  const rovingChangePath = workspaceChangeRovingPath(
    visibleInspectablePaths,
    model.changes.selectedFile,
    focusedChangePath,
  );
  const navigation = workspaceChangeNavigation(inspectablePaths, model.changes.selectedFile);
  const missingSourceProjection = workspaceChoiceProjection(
    model.changes.structural?.missingSources ?? [],
    missingSourceQuery,
    model.changes.structural?.missingSource ?? "",
  );
  const moveTargetProjection = workspaceChoiceProjection(
    model.changes.structural?.moveTargets ?? [],
    moveTargetQuery,
    model.changes.structural?.moveTarget ?? "",
  );
  useEffect(() => {
    if (focusedChangePath === rovingChangePath) return;
    setFocusedChangePath(rovingChangePath);
  }, [focusedChangePath, rovingChangePath]);
  const editorVisible = model.changes.editorKind !== "none";
  const selectedName = model.changes.selectedFile.split("/").at(-1) || "No file selected";
  const statusTone = (code: "A" | "D" | "M" | "?") => ({
    A: "text-emerald-300 bg-emerald-400/10 border-emerald-400/25",
    D: "text-red-300 bg-red-400/10 border-red-400/25",
    M: "text-amber-300 bg-amber-400/10 border-amber-400/25",
    "?": "text-muted-foreground bg-muted border-border",
  }[code]);
  const inspect = (path: string) => {
    if (!model.changes.canSelectFile || !loadFile?.enabled || !editablePaths.has(path)) return;
    onIntent({ type: "activate", action: "load-file", field: "selectedFile", value: path });
  };
  const focusChange = (path: string) => {
    setFocusedChangePath(path);
    window.requestAnimationFrame(() => changeButtonRefs.current.get(path)?.focus());
  };
  const moveChangeFocus = (event: ReactKeyboardEvent<HTMLButtonElement>, path: string) => {
    if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key) || inspectablePaths.length === 0) return;
    event.preventDefault();
    const nextPath = workspaceChangeFocusNavigation(
      inspectablePaths,
      path,
      event.key as "ArrowUp" | "ArrowDown" | "Home" | "End",
    );
    if (!nextPath) return;
    inspect(nextPath);
    focusChange(nextPath);
  };
  return (
    <div className="grid" aria-label="Changes and text editor">
      <header className="flex min-h-16 flex-wrap items-center justify-between gap-3 border-b border-border bg-background/35 px-4 py-2">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-base font-semibold tracking-tight">Working changes</h2>
            <Badge tone={changeRows.length ? "warning" : "neutral"}>{model.changes.queueSummary}</Badge>
            <Badge tone="neutral">LOCAL + AUTHENTICATED</Badge>
          </div>
          <p className="mt-0.5 text-xs text-muted-foreground">Review the working set, compare exact bytes, then save selected work privately.</p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <p className="flex items-center gap-3 font-mono text-[11px]" aria-label="Change summary">
            <span className="text-amber-300">{changeCounts.M} modified</span>
            <span className="text-emerald-300">{changeCounts.A} added</span>
            <span className="text-red-300">{changeCounts.D} deleted</span>
          </p>
          <WorkAction id="scan-files" actions={actions} onIntent={onIntent} />
        </div>
      </header>

      <div className={`grid min-h-[42rem] overflow-hidden bg-card ${changesPanelOpen ? "lg:grid-cols-[21rem_minmax(0,1fr)]" : "lg:grid-cols-[3.25rem_minmax(0,1fr)]"}`}>
        {changesPanelOpen ? <section id="workspace-changes-panel" className="grid min-w-0 grid-rows-[auto_auto_minmax(0,1fr)_auto] border-b border-border bg-background/55 lg:border-b-0 lg:border-r" aria-labelledby="native-queue-heading" data-mesh-native-queue tabIndex={-1}>
          <div className="flex min-h-14 items-center justify-between border-b border-border px-3">
            <div>
              <h3 id="native-queue-heading" className="text-xs font-semibold uppercase tracking-[0.14em]">Source control</h3>
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                {changeProjection.truncated
                  ? `${changeProjection.offset + 1}–${changeProjection.offset + changeProjection.displayed} of ${changeProjection.matched} visible changes shown`
                  : `${changeProjection.matched} of ${changeRows.length} visible`}
              </p>
            </div>
            <div className="flex items-center gap-1">
              <span className="rounded-md bg-muted px-2 py-1 font-mono text-xs text-muted-foreground">WORKING</span>
              <Button className="px-2" size="compact" variant="quiet" aria-controls="workspace-changes-panel" aria-expanded="true" onClick={() => setChangesPanelOpen(false)}>Hide</Button>
            </div>
          </div>

          <div className="grid gap-2 border-b border-border p-3">
            <label className="relative block">
              <span className="sr-only">Search working changes</span>
              <span className="pointer-events-none absolute inset-y-0 left-3 grid place-items-center text-muted-foreground"><SearchGlyph /></span>
              <input
                type="search"
                value={changeQuery}
                onChange={(event) => setChangeQuery(event.currentTarget.value)}
                placeholder="Filter changed files…"
                className="min-h-11 w-full rounded-md border border-border bg-background py-2 pl-9 pr-3 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
              />
            </label>
            <div className="flex gap-1 overflow-x-auto" aria-label="Filter changes by status">
              {([
                ["all", "All", "All changes"],
                ["M", "Modified", "Modified changes"],
                ["A", "Added", "Added changes"],
                ["D", "Deleted", "Deleted or moved changes"],
                ["?", "Unsupported", "Unsupported changes"],
              ] as const).map(([filter, visibleLabel, label]) => (
                <Button
                  key={filter}
                  size="compact"
                  variant={changeFilter === filter ? "secondary" : "quiet"}
                  className="shrink-0 px-2.5"
                  aria-label={`${label}, ${changeCounts[filter]}`}
                  aria-pressed={changeFilter === filter}
                  data-mesh-change-filter={filter}
                  onClick={() => setChangeFilter(filter)}
                >
                  <span aria-hidden="true">{visibleLabel}</span>
                  <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px]" aria-hidden="true">{changeCounts[filter]}</span>
                </Button>
              ))}
            </div>
          </div>

          <div className="max-h-[31rem] overflow-y-auto p-2">
            {changeGroups.length ? (
              <div className="grid gap-3" role="listbox" aria-label="Working changes">
                {changeGroups.map((group) => (
                  <section key={group.folder || "workspace-root"} role="group" aria-label={group.folder || "Workspace root"}>
                    <div className="flex min-h-8 items-center justify-between gap-2 px-2 text-[11px] font-semibold uppercase tracking-[0.1em] text-muted-foreground">
                      <span className="truncate normal-case tracking-normal">{group.folder || "Workspace root"}</span>
                      <span className="font-mono">{group.changes.length}{changeProjection.truncated ? " shown" : ""}</span>
                    </div>
                    <ul className="grid gap-px" role="presentation">
                      {group.changes.map((change) => {
                        const inspectable = editablePaths.has(change.path) && Boolean(loadFile?.enabled) && model.changes.canSelectFile;
                        const selected = model.changes.selectedFile === change.path;
                        const name = change.path.split("/").at(-1) || change.path;
                        return (
                          <li key={change.path} role="presentation">
                            <button
                              type="button"
                              ref={(element) => {
                                if (element) changeButtonRefs.current.set(change.path, element);
                                else changeButtonRefs.current.delete(change.path);
                              }}
                              role="option"
                              className={`group flex min-h-11 w-full items-center gap-2 rounded-md px-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring disabled:cursor-default ${selected ? "bg-sky-300/10 text-foreground" : "text-muted-foreground hover:bg-muted/70"}`}
                              disabled={!inspectable}
                              aria-selected={selected}
                              tabIndex={inspectable && rovingChangePath === change.path ? 0 : -1}
                              aria-label={`${change.status}: ${change.path}. ${change.detail || change.description}${inspectable ? "" : ". Preview unavailable"}`}
                              onClick={() => {
                                setFocusedChangePath(change.path);
                                inspect(change.path);
                              }}
                              onKeyDown={(event) => moveChangeFocus(event, change.path)}
                            >
                              <span className={`grid size-6 shrink-0 place-items-center rounded border font-mono text-xs font-bold ${statusTone(change.code)}`} data-mesh-change-code={change.code}>{change.code}</span>
                              <span className="min-w-0 flex-1">
                                <span className="block truncate font-mono text-[13px] text-foreground">{name}</span>
                                <span className="mt-0.5 block truncate text-[11px]">{change.detail || change.description}</span>
                              </span>
                              {inspectable ? <Chevron /> : null}
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </section>
                ))}
              </div>
            ) : <div className="grid min-h-40 place-items-center p-5 text-center"><div><p className="text-sm font-semibold">{changeRows.length ? "No matching changes" : model.changes.scanState === "clean" ? "No working changes" : model.changes.scanState === "error" ? "Folder check incomplete" : model.changes.scanState === "scanning" ? "Checking folder…" : "Check the working folder"}</p><p className="mt-1 text-xs leading-5 text-muted-foreground">{changeRows.length ? "Adjust the search or status filter." : model.changes.scanState === "clean" ? "The native folder matches private history exactly." : model.changes.scanState === "error" ? "Retry the folder check before saving or review." : model.changes.scanState === "scanning" ? "Mesh is comparing current bytes with private history." : "Run a folder check to compare current bytes with private history."}</p>{!changeRows.length && model.changes.scanState !== "scanning" ? <div className="mt-3"><WorkAction id="scan-files" actions={actions} onIntent={onIntent} variant="primary" /></div> : null}</div></div>}

            {changeProjection.truncated ? (
              <p className="mt-3 rounded-md border border-amber-400/25 bg-amber-400/5 p-3 text-xs leading-5 text-amber-100" role="status">
                Showing changes {changeProjection.offset + 1}–{changeProjection.offset + changeProjection.displayed} of {changeProjection.matched}. Refine the change search or status filter to inspect a narrower result set.
              </p>
            ) : null}

            {model.changes.structural ? (
              <details className="mt-3 rounded-md border border-amber-400/25 bg-amber-400/5">
                <summary className="flex min-h-11 cursor-pointer items-center gap-2 px-3 text-xs font-semibold text-amber-200"><Chevron /> Resolve missing file</summary>
                <div className="grid gap-3 border-t border-amber-400/20 p-3">
                  <div className="grid gap-1.5 text-xs font-medium">
                    <label htmlFor="changes-missing-source-filter">Find a missing tracked file</label>
                    <input id="changes-missing-source-filter" type="search" value={missingSourceQuery} onChange={(event) => setMissingSourceQuery(event.currentTarget.value)} placeholder="Filter missing files…" className="min-h-11 min-w-0 rounded-md border border-border bg-background px-2 text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring" />
                    <label htmlFor="changes-missing-source">Missing tracked file</label>
                    <select id="changes-missing-source" ref={missingSourceRef} className="min-h-11 min-w-0 rounded-md border border-border bg-background px-2 text-foreground" value={model.changes.structural.missingSource} data-mesh-work-field="missingSource" disabled={!model.changes.structural.canChoose} onChange={(event) => onIntent({ type: "set-field", field: "missingSource", value: event.currentTarget.value })}>
                      {missingSourceProjection.items.map((item) => <option data-mesh-structural-source="true" key={item.value} value={item.value}>{item.label}</option>)}
                    </select>
                    <span className="font-normal text-muted-foreground">{structuralProjectionCopy(missingSourceProjection, "missing files")}</span>
                  </div>
                  <div className="grid gap-1.5 text-xs font-medium">
                    <label htmlFor="changes-move-target-filter">Find a possible destination</label>
                    <input id="changes-move-target-filter" type="search" value={moveTargetQuery} onChange={(event) => setMoveTargetQuery(event.currentTarget.value)} placeholder="Filter possible destinations…" className="min-h-11 min-w-0 rounded-md border border-border bg-background px-2 text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring" />
                    <label htmlFor="changes-move-target">Resolution</label>
                    <select id="changes-move-target" ref={moveTargetRef} className="min-h-11 min-w-0 rounded-md border border-border bg-background px-2 text-foreground" value={model.changes.structural.moveTarget} data-mesh-work-field="moveTarget" disabled={!model.changes.structural.canChoose} onChange={(event) => onIntent({ type: "set-field", field: "moveTarget", value: event.currentTarget.value })}>
                      <option value="">It was deleted</option>
                      {moveTargetProjection.items.map((item) => <option data-mesh-structural-target="true" key={item.value} value={item.value}>{item.label}</option>)}
                    </select>
                    <span className="font-normal text-muted-foreground">{structuralProjectionCopy(moveTargetProjection, "possible destinations")}</span>
                  </div>
                  <WorkAction id="record-structural-change" actions={actions} onIntent={onIntent} activationIntent={() => ({ type: "activate", action: "record-structural-change", missingSource: missingSourceRef.current?.value ?? model.changes.structural?.missingSource ?? "", moveTarget: moveTargetRef.current?.value ?? model.changes.structural?.moveTarget ?? "" })} />
                  <p className="text-xs leading-5 text-muted-foreground">{model.changes.structural.hint}</p>
                </div>
              </details>
            ) : null}
          </div>

          <div className="grid gap-3 border-t border-border p-3">
            <WorkAction id="save-all-private" actions={actions} onIntent={onIntent} variant="primary" />
            <label className="flex min-h-11 items-start gap-2 rounded-md border border-border bg-muted/20 p-2.5 text-xs">
              <input className="mt-0.5 size-4" type="checkbox" checked={model.changes.autoSaveChecked} disabled={!model.changes.autoSaveEnabled} onChange={(event) => onIntent({ type: "set-auto-save", checked: event.currentTarget.checked })} />
              <span><strong>Auto-save safe changes</strong><span className="mt-1 block leading-4 text-muted-foreground">{model.changes.autoSaveHint}</span></span>
            </label>
          </div>
        </section> : <aside id="workspace-changes-panel" className="flex min-h-12 items-start justify-center border-b border-border bg-background/55 p-2 lg:border-b-0 lg:border-r" aria-label="Source control collapsed">
          <Button className="px-2 lg:[writing-mode:vertical-rl]" size="compact" variant="quiet" aria-controls="workspace-changes-panel" aria-expanded="false" onClick={() => setChangesPanelOpen(true)}>Show Changes</Button>
        </aside>}

        <section className="grid min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] bg-[#0b1016]" aria-labelledby="file-editor-heading">
          <div className="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-border bg-background/30 px-4 py-2">
            <div className="flex min-w-0 items-center gap-2">
              <span className="grid size-8 shrink-0 place-items-center rounded-md border border-border bg-muted"><FileGlyph node={{ kind: "file", path: model.changes.selectedFile }} /></span>
              <div className="min-w-0">
                <h3 id="file-editor-heading" className="truncate font-mono text-sm font-semibold">{selectedName}</h3>
                <p className="truncate font-mono text-[11px] text-muted-foreground">{model.changes.selectedFile || "Select a change to inspect"}</p>
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <div className="flex items-center" aria-label="Navigate inspectable changes">
                <Button
                  size="compact"
                  variant="quiet"
                  className="rounded-r-none px-2.5"
                  disabled={!navigation.previous}
                  data-mesh-change-navigation="previous"
                  onClick={() => navigation.previous && inspect(navigation.previous)}
                >Previous</Button>
                <Button
                  size="compact"
                  variant="quiet"
                  className="rounded-l-none border-l border-border px-2.5"
                  disabled={!navigation.next}
                  data-mesh-change-navigation="next"
                  onClick={() => navigation.next && inspect(navigation.next)}
                >Next</Button>
              </div>
              {model.changes.editorKind === "text" ? <SegmentedControl label="Text workspace view" value={view} onChange={(value) => setView(value as "Edit" | "Inline" | "Split")} options={["Edit", "Inline", "Split"]} /> : null}
            </div>
          </div>

          <div className="min-h-0 overflow-auto bg-background/20 p-3">
            {editorVisible ? (
              model.changes.editorKind === "text" && view === "Edit" ? (
                <label className="grid gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  Inspected working text
                  <textarea ref={editorRef} className="min-h-[27rem] w-full resize-y rounded-md border border-border bg-[#070b10] p-4 font-mono text-[13px] font-normal leading-6 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" spellCheck={false} value={model.changes.editorText} disabled={!model.changes.canEditText} onChange={(event) => onIntent({ type: "set-field", field: "editorText", value: event.currentTarget.value })} />
                </label>
              ) : model.changes.editorKind === "text" ? (
                <TextComparison before={model.changes.baselineText} after={model.changes.editorText} split={view === "Split"} baselineAvailable={model.changes.baselineAvailable} />
              ) : (
                <div className="grid min-h-72 place-items-center rounded-md border border-dashed border-border p-8 text-center"><div><p className="font-semibold">Preview unavailable</p><p className="mt-2 max-w-md text-sm leading-6 text-muted-foreground">Binary or large content stays outside the text editor. Mesh still tracks and saves it by exact identity.</p></div></div>
              )
            ) : <div className="grid min-h-72 place-items-center text-center"><div className="max-w-lg"><p className="font-semibold">{changeRows.length ? "Select a working change" : model.changes.scanState === "clean" ? "No working changes" : model.changes.scanState === "error" ? "Folder check incomplete" : "Review current folder changes"}</p><p className="mt-2 text-sm leading-6 text-muted-foreground">{changeRows.length ? "Choose an inspectable file from Source control to see its exact inline or split diff." : model.changes.scanState === "clean" ? "The native folder matches the latest private version. Edit a file in your usual app, then check again." : model.changes.scanState === "error" ? "Mesh could not complete an exact comparison. Retry before trusting or saving this view." : "Check the native folder, then select a changed file to see its exact inline or split diff here."}</p>{!changeRows.length && model.changes.scanState !== "scanning" ? <div className="mt-4"><WorkAction id="scan-files" actions={actions} onIntent={onIntent} variant="primary" /></div> : null}</div></div>}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border bg-background/35 px-4 py-3">
            <p className="text-xs text-muted-foreground" role="status" aria-live="polite"><span className="text-foreground">{model.changes.editState}</span> · <code>{model.changes.editVersion}</code></p>
            <div className="flex flex-wrap gap-2">
              <WorkAction id="preserve-edit" actions={actions} onIntent={onIntent} activationEcho={() => ({ field: "editorText", value: editorRef.current?.value ?? model.changes.editorText })} />
              <WorkAction id="save-private" actions={actions} onIntent={onIntent} variant="primary" />
            </div>
          </div>
        </section>
      </div>

      <details className="border-t border-border bg-background/35">
        <summary className="flex min-h-11 cursor-pointer items-center gap-2 px-4 text-sm font-semibold"><Chevron /> How Mesh handles working changes</summary>
        <div className="grid gap-3 border-t border-border p-4 text-xs leading-5 text-muted-foreground">
          <p>Edit here or work normally in the stable native folder with a local editor. Give Terminal or a long-running agent the independent agent folder shown under Current; it starts at the selected durable version and then remains writable even if Mesh switches elsewhere. Returning to Mesh automatically inspects the selected folder; selecting a different recent agent folder inspects it immediately, while inactive agent folders are not continuously watched. Find folder changes retries the read explicitly.</p>
          <p>Inspect re-reads exact operating-system bytes; binary and large files stay outside the text editor but remain savable. Choose Save privately to sign and append one inspected file. Mesh reviews a complete new folder tree and admits its folders parent-first. Outside the confirmed agent-finish flow, saving remains an explicit authenticated action. Choose Save all privately for the complete inspected queue. Automatic save uses the same complete scan, reinspection, signature, and post-save verification. It pauses for deletions, renames, symbolic links, special entries, active agent handoffs, or files that keep changing. Mesh stops on the first mismatch.</p>
        </div>
      </details>
    </div>
  );
}

function structuralProjectionCopy(
  projection: ReturnType<typeof workspaceChoiceProjection>,
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

export function TextComparison({ before, after, split, baselineAvailable = true }: { before: string; after: string; split: boolean; baselineAvailable?: boolean }) {
  const comparison = useMemo(
    () => baselineAvailable ? workspaceTextDiff(before, after) : null,
    [after, baselineAvailable, before],
  );
  if (!baselineAvailable) {
    return <section className="grid min-h-64 place-items-center rounded-md border border-dashed border-border p-8 text-center" aria-label="Text comparison unavailable">
      <div className="max-w-lg">
        <h4 className="font-semibold">Saved baseline cannot be displayed</h4>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">The saved baseline is not available as bounded text. The exact working file remains available in Edit.</p>
      </div>
    </section>;
  }
  if (!comparison) return null;
  if (comparison.kind === "unavailable") {
    return <section className="grid min-h-64 place-items-center rounded-md border border-dashed border-border p-8 text-center" aria-label="Text comparison unavailable">
      <div className="max-w-lg">
        <h4 className="font-semibold">Comparison is too large to display</h4>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">{comparison.reason} The exact file remains available in Edit.</p>
      </div>
    </section>;
  }
  if (comparison.kind === "unchanged") {
    return <section className="grid min-h-64 place-items-center rounded-md border border-dashed border-border p-8 text-center" aria-label="Unchanged text comparison">
      <div><h4 className="font-semibold">No text changes</h4><p className="mt-2 text-sm text-muted-foreground">The working copy matches the saved version exactly.</p></div>
    </section>;
  }
  return <section className="min-w-0 overflow-hidden rounded-md border border-border bg-[#070b10]" aria-label={split ? "Split text comparison" : "Inline text comparison"}>
    <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-muted/25 px-3 py-2">
      <h4 className="text-xs font-semibold">Working tree diff</h4>
      <p className="flex items-center gap-3 font-mono text-[11px]">
        <span className="text-emerald-300">+{comparison.additions} {comparison.additions === 1 ? "addition" : "additions"}</span>
        <span className="text-red-300">−{comparison.deletions} {comparison.deletions === 1 ? "deletion" : "deletions"}</span>
      </p>
    </div>
    <div className="max-h-[32rem] overflow-auto">
      {split ? <SplitWorkingDiff hunks={comparison.hunks} /> : <InlineWorkingDiff hunks={comparison.hunks} />}
    </div>
  </section>;
}

function diffLineTone(line: WorkspaceTextDiffLine | null, counterpart: WorkspaceTextDiffLine | null = null): string {
  if (line?.kind === "removed" || (!line && counterpart?.kind === "removed")) return "bg-red-400/10 text-red-100";
  if (line?.kind === "added" || (!line && counterpart?.kind === "added")) return "bg-emerald-400/10 text-emerald-100";
  return "text-foreground";
}

function diffLineMark(line: WorkspaceTextDiffLine | null): string {
  if (line?.kind === "removed") return "−";
  if (line?.kind === "added") return "+";
  return " ";
}

function diffLineStatus(line: WorkspaceTextDiffLine): string {
  if (line.kind === "removed") return "Removed";
  if (line.kind === "added") return "Added";
  return "Unchanged";
}

function DiffLineEnding({ line }: { line: WorkspaceTextDiffLine }) {
  if (line.ending === "lf" || (line.ending === "crlf" && line.kind === "context")) return null;
  return <span className="ml-3 select-none rounded border border-border bg-muted/50 px-1.5 py-0.5 font-sans text-[9px] uppercase tracking-wide text-muted-foreground">
    {line.ending === "none" ? "No newline" : "CRLF"}
  </span>;
}

function HunkHeader({ hunk }: { hunk: WorkspaceTextDiffHunk }) {
  return <div className="border-y border-border bg-sky-400/5 px-3 py-2 font-mono text-[11px] text-sky-200">
    <span className="sr-only">Changed lines. </span>
    @@ -{hunk.beforeStart},{hunk.beforeCount} +{hunk.afterStart},{hunk.afterCount} @@
  </div>;
}

function InlineWorkingDiff({ hunks }: { hunks: readonly WorkspaceTextDiffHunk[] }) {
  return <div className="min-w-max font-mono text-[13px] leading-6">
    {hunks.map((hunk, hunkIndex) => <div key={`${hunk.beforeStart}:${hunk.afterStart}:${hunkIndex}`}>
      <HunkHeader hunk={hunk} />
      {hunk.lines.map((line, index) => <div className={`grid min-h-6 grid-cols-[3rem_3rem_2rem_minmax(0,1fr)] ${diffLineTone(line)}`} key={`${line.kind}:${line.before}:${line.after}:${index}`} data-mesh-work-diff-line={line.kind}>
        <span className="sr-only">{diffLineStatus(line)}. {line.before === null ? "No earlier line" : `Earlier line ${line.before}`}. {line.after === null ? "No current line" : `Current line ${line.after}`}. </span>
        <span aria-hidden="true" className="select-none border-r border-border/60 px-2 text-right text-muted-foreground/70">{line.before ?? ""}</span>
        <span aria-hidden="true" className="select-none border-r border-border/60 px-2 text-right text-muted-foreground/70">{line.after ?? ""}</span>
        <span aria-hidden="true" className="select-none text-center">{diffLineMark(line)}</span>
        <code className="whitespace-pre-wrap break-words px-2">{line.text || " "}<DiffLineEnding line={line} /></code>
      </div>)}
    </div>)}
  </div>;
}

function SplitWorkingDiff({ hunks }: { hunks: readonly WorkspaceTextDiffHunk[] }) {
  return <div className="min-w-[48rem] font-mono text-[13px] leading-6">
    <div className="grid grid-cols-2 border-b border-border bg-muted/20 font-sans text-xs font-semibold text-muted-foreground">
      <span className="px-3 py-2">Saved version</span>
      <span className="border-l border-border px-3 py-2">Working copy</span>
    </div>
    {hunks.map((hunk, hunkIndex) => <div key={`${hunk.beforeStart}:${hunk.afterStart}:${hunkIndex}`}>
      <HunkHeader hunk={hunk} />
      {workspaceSplitDiffRows(hunk.lines).map((row, index) => <div className="grid grid-cols-2" key={`${hunkIndex}:${index}`}>
        <SplitDiffSide side="Earlier" line={row.before} counterpart={row.after} />
        <SplitDiffSide side="Current" line={row.after} counterpart={row.before} right />
      </div>)}
    </div>)}
  </div>;
}

function SplitDiffSide({ side, line, counterpart, right = false }: {
  side: "Earlier" | "Current";
  line: WorkspaceTextDiffLine | null;
  counterpart: WorkspaceTextDiffLine | null;
  right?: boolean;
}) {
  const number = side === "Earlier" ? line?.before : line?.after;
  return <div className={`grid min-h-6 grid-cols-[3rem_2rem_minmax(0,1fr)] ${right ? "border-l border-border" : ""} ${diffLineTone(line, counterpart)}`}>
    <span className="sr-only">{line ? `${side} line ${number}. ${diffLineStatus(line)}. ` : `No corresponding ${side.toLocaleLowerCase()} line. `}</span>
    <span aria-hidden="true" className="select-none border-r border-border/60 px-2 text-right text-muted-foreground/70">{number ?? ""}</span>
    <span aria-hidden="true" className="select-none text-center">{diffLineMark(line)}</span>
    <code className="whitespace-pre-wrap break-words px-2">{line ? line.text || " " : " "}{line ? <DiffLineEnding line={line} /> : null}</code>
  </div>;
}

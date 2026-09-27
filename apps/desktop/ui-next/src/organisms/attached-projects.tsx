import { useTranslation } from "../lib/localization";
import { useEffect, useState } from "react";
import { Button } from "../atoms/button";

type Project = { id: string; generation: string; root: string; phase: string; outcome: string; savedVersion: string | null; captureAgeMs: number | null };
type SavedEntry = { path: string; kind: "file" | "folder"; bytes: number | null; digest: string | null; executable: boolean | null };
type SavedFile = { path: string; state: "text" | "binary" | "too-large"; text: string | null; bytes: number };
type Inspection = { operation: string; entries: SavedEntry[]; nextAfter: string | null; file: SavedFile | null };
type VersionPage = { versions: string[]; nextBefore: string | null };
type Change = { path: string; change: string; before: Omit<SavedEntry, "path"> | null; after: Omit<SavedEntry, "path"> | null };
type Comparison = { base: string; target: string; changes: Change[]; total: number; nextAfter: string | null;
  file: { path: string; before: SavedFile | null; after: SavedFile | null; beforeKind: string; afterKind: string } | null };
type Projection = { bases: Record<string, string>; comparisons: Record<string, Comparison>; inspections: Record<string, Inspection>; histories: Record<string, VersionPage>; projects: Project[]; busy: boolean; error: string; available: boolean };
const empty: Projection = { bases: {}, comparisons: {}, inspections: {}, histories: {}, projects: [], busy: false, error: "", available: false };
const send = (detail: Record<string, string | null>) => document.dispatchEvent(new CustomEvent("mesh:attachments-intent", { detail }));
const phases: Record<string, string> = {
  starting: "Starting capture", scanning: "Checking changes", saving: "Saving a version",
  waiting: "Watching for changes", stopping: "Stopping capture", stopped: "Capture stopped", failed: "Capture needs attention",
};
const outcomes: Record<string, string> = {
  pending: "First capture pending", saved: "Version saved", unchanged: "No new changes",
  incomplete: "Capture incomplete", "source-unavailable": "Project folder unavailable",
  "store-unavailable": "History storage unavailable", "save-unavailable": "Version could not be saved", cancelled: "Capture cancelled",
};

export function AttachedProjects() {
  const t = useTranslation();
  const [projection, setProjection] = useState<Projection>(empty);
  const [source, setSource] = useState("");
  useEffect(() => {
    const update = (event: Event) => setProjection((event as CustomEvent<Projection>).detail);
    document.addEventListener("mesh:attachments-projection", update);
    document.dispatchEvent(new CustomEvent("mesh:attachments-visible", { detail: true }));
    return () => {
      document.removeEventListener("mesh:attachments-projection", update);
      document.dispatchEvent(new CustomEvent("mesh:attachments-visible", { detail: false }));
    };
  }, []);
  const disabled = projection.busy || !projection.available || Boolean(projection.error);
  return <section className="grid gap-4 rounded-xl border border-border bg-background p-5" aria-label={t("Attached projects")} data-mesh-proof="attached-projects">
    <div>
      <h3 className="text-xl font-semibold">{t("Use your existing project")}</h3>
      <p className="mt-2 text-sm leading-6 text-muted-foreground">{t("Keep your folder, editor, Git workflow and agent sessions where they are. Mesh saves history in separate storage while you work.")}</p>
    </div>
    <Button disabled={projection.busy || !projection.available} onClick={() => send({ type: "choose" })}>{t("Choose project to attach")}</Button>
    <label className="grid gap-2 text-sm font-medium">{t("Existing project path")}
      <input dir="ltr" className="min-h-11 rounded-md border border-border bg-background px-3" value={source}
        onChange={(event) => setSource(event.target.value)} placeholder="/Users/you/Project" autoComplete="off" />
    </label>
    <Button disabled={projection.busy || !projection.available || !source.startsWith("/")}
      onClick={() => send({ type: "attach", source })}>{t("Attach existing project")}</Button>
    {!projection.available && <p className="text-sm text-muted-foreground">{t("Attachment requires the native Mesh desktop.")}</p>}
    {projection.error && <p role="alert" className="text-sm">{t(projection.error)}</p>}
    <div className="flex items-center gap-3">
      <Button variant="secondary" disabled={projection.busy || !projection.available} onClick={() => send({ type: "refresh" })}>{t("Refresh status")}</Button>
      <span role="status" className="text-sm text-muted-foreground">{t(projection.busy ? "Updating…" : projection.error ? "Status may be out of date" : "")}</span>
    </div>
    {projection.projects.map((project) => {
      const terminal = project.phase === "stopped" || project.phase === "failed";
      return <article key={project.id} className="grid gap-2 rounded-lg border border-border p-4">
        <p className="break-all text-sm font-medium"><bdi dir="ltr">{project.root}</bdi></p>
        <p className="text-sm">{t(projection.error ? "Status may be out of date" : phases[project.phase])} · {t(outcomes[project.outcome])}</p>
        <p className="break-all text-xs text-muted-foreground">{project.savedVersion ? <>{t("Latest saved version:")} <bdi dir="ltr">{project.savedVersion}</bdi></> : t("No saved version yet")}</p>
        <p className="text-xs text-muted-foreground">{project.captureAgeMs === null ? t("No complete capture in this session") : <>{t("Last complete capture started")} {Math.floor(project.captureAgeMs / 1000)} {t("seconds ago")}</>}. {t("Change author unknown.")}</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" disabled={disabled || !project.savedVersion}
            onClick={() => send({ type: "versions", id: project.id, before: null })}>{t("Show latest versions")}</Button>
          <Button variant="secondary" disabled={disabled || terminal || project.phase === "stopping"}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: "capture" })}>{t("Capture now")}</Button>
          <Button variant="secondary" disabled={disabled || project.phase === "stopping"}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: terminal ? "resume" : "stop" })}>{t(terminal ? "Resume capture" : "Stop capture")}</Button>
        </div>
        {projection.histories[project.id] && <section aria-label={`${t("Saved versions for")} \u2068${project.root}\u2069`} className="grid gap-2">
          <p className="text-sm font-medium">{t("Saved versions · newest first")}</p>
          <p className="break-all text-xs text-muted-foreground">{projection.bases[project.id] && <>{t("Selected comparison base:")} <bdi dir="ltr">{projection.bases[project.id]}</bdi></>}</p>
          <ol className="max-h-64 overflow-auto text-xs">
            {projection.histories[project.id].versions.map((version) => <li key={version} className="break-all border-b border-border py-2"><button className="text-left underline" disabled={disabled}
              onClick={() => send({ type: "inspect", id: project.id, operation: version })}><bdi dir="ltr">{version}</bdi></button>
              <div className="mt-1 flex gap-3"><button className="underline" disabled={disabled}
                onClick={() => send({ type: "set-base", id: project.id, operation: version })}>{t("Use as base")}</button>
              <button className="underline" disabled={disabled || !projection.bases[project.id]}
                onClick={() => send({ type: "compare", id: project.id, target: version })}>{t("Compare with base")}</button></div></li>)}
          </ol>
          {projection.histories[project.id].nextBefore && <Button variant="secondary" disabled={disabled}
            onClick={() => send({ type: "versions", id: project.id, before: projection.histories[project.id].nextBefore })}>{t("Older versions")}</Button>}
          <p className="text-xs text-muted-foreground">{t("This history page stays fixed while capture continues.")}</p>
        </section>}
        {projection.comparisons[project.id] && <SavedComparison project={project.id} comparison={projection.comparisons[project.id]} disabled={disabled} />}
        {projection.inspections[project.id] && <SavedInspection project={project.id} inspection={projection.inspections[project.id]} disabled={disabled} />}
      </article>;
    })}
    <p className="text-xs text-muted-foreground">{t("After reopening Mesh, attach the same project to resume capture. Saved history is retained.")}</p>
  </section>;
}

function SavedInspection({ project, inspection, disabled }: { project: string; inspection: Inspection; disabled: boolean }) {
  const t = useTranslation();
  return <section aria-label={t("Saved version files")} className="grid gap-3 rounded-lg border border-border p-3">
    <h4 className="text-sm font-semibold">{t("Files in saved version")}</h4>
    <p className="break-all text-xs text-muted-foreground"><bdi dir="ltr">{inspection.operation}</bdi></p>
    <ul className="max-h-64 overflow-auto text-sm">
      {inspection.entries.map((entry) => <li key={entry.path} className="break-all py-1">
        {entry.kind === "folder" ? <bdi dir="ltr">{entry.path}/</bdi> : <button className="text-left underline" disabled={disabled}
          onClick={() => send({ type: "file", id: project, operation: inspection.operation, path: entry.path })}><bdi dir="ltr">{entry.path}</bdi> · {entry.bytes} {t("Bytes")}{entry.executable ? <> · {t("Executable")}</> : ""}</button>}
      </li>)}
    </ul>
    {inspection.entries.length === 0 && <p className="text-sm">{t("This saved version has no files or folders.")}</p>}
    {inspection.nextAfter && <Button variant="secondary" disabled={disabled}
      onClick={() => send({ type: "entries", id: project, operation: inspection.operation, after: inspection.nextAfter })}>{t("More files")}</Button>}
    {inspection.file && <div className="grid gap-2">
      <p className="break-all text-sm font-medium"><bdi dir="ltr">{inspection.file.path}</bdi></p>
      {inspection.file.state === "text" ? <pre dir="ltr" className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 text-xs">{inspection.file.text}</pre>
        : <p className="text-sm text-muted-foreground">{t(inspection.file.state === "binary" ? "Binary file. Text preview is unavailable." : "This file exceeds the 256 KiB text preview limit.")}</p>}
    </div>}
    <p className="text-xs text-muted-foreground">{t("Read-only saved content. Edits in your working folder do not change this view.")}</p>
  </section>;
}

function SavedComparison({ project, comparison, disabled }: { project: string; comparison: Comparison; disabled: boolean }) {
  const t = useTranslation();
  const labels: Record<string, string> = { added: "Added", removed: "Removed", modified: "Content changed", "mode-changed": "Executable mode changed", "type-changed": "File/folder type changed" };
  const describe = (side: Change["before"]) => side?.kind === "file"
    ? <>{side.bytes} {t("Bytes")}, {t(side.executable ? "Executable" : "Not executable")}</>
    : t(side ? "Folder" : "Absent in this version");
  return <section aria-label={t("Saved version comparison")} className="grid gap-3 rounded-lg border border-border p-3">
    <h4 className="text-sm font-semibold">{t("Compare saved versions")} · {comparison.total} {t("changed paths")}</h4>
    <p className="break-all text-xs text-muted-foreground">{t("Base:")} <bdi dir="ltr">{comparison.base}</bdi></p>
    <p className="break-all text-xs text-muted-foreground">{t("Compared version:")} <bdi dir="ltr">{comparison.target}</bdi></p>
    <ul className="max-h-64 overflow-auto text-sm">{comparison.changes.map((change) => <li key={change.path} className="py-1">
      <button className="break-all text-left underline" disabled={disabled}
        onClick={() => send({ type: "compare-file", id: project, base: comparison.base, target: comparison.target, path: change.path })}>{t(labels[change.change])} · <bdi dir="ltr">{change.path}</bdi></button>
      <span className="block text-xs text-muted-foreground">{t("Before")}: {describe(change.before)} · {t("After")}: {describe(change.after)}</span>
    </li>)}</ul>
    {comparison.total === 0 && <p className="text-sm">{t("These versions have the same paths, content and executable modes.")}</p>}
    {comparison.nextAfter && <Button variant="secondary" disabled={disabled}
      onClick={() => send({ type: "compare-page", id: project, base: comparison.base, target: comparison.target, after: comparison.nextAfter })}>{t("More changes")}</Button>}
    {comparison.file && <div className="grid gap-3">
      <p className="break-all text-sm font-medium"><bdi dir="ltr">{comparison.file.path}</bdi></p>
      <div className="grid gap-3 lg:grid-cols-2">
        <ComparisonSide label="Before" file={comparison.file.before} kind={comparison.file.beforeKind} />
        <ComparisonSide label="After" file={comparison.file.after} kind={comparison.file.afterKind} />
      </div>
    </div>}
    <p className="text-xs text-muted-foreground">{t("This comparison stays on these saved versions while work continues. It does not approve or apply changes.")}</p>
  </section>;
}
function ComparisonSide({ label, file, kind }: { label: string; file: SavedFile | null; kind: string }) {
  const t = useTranslation();
  return <section className="min-w-0" aria-label={t(label)}><h5 className="text-sm font-medium">{t(label)}</h5>
    {file?.state === "text" ? <pre dir="ltr" className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 text-xs">{file.text}</pre>
      : <p className="text-sm text-muted-foreground">{t(!file ? kind === "absent" ? "Absent in this version" : "Folder" : file.state === "binary" ? "Binary file; no text preview" : "File exceeds the 256 KiB preview limit")}</p>}
  </section>;
}

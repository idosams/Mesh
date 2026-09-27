import { useTranslation } from "../lib/localization";
import { useEffect, useState } from "react";
import { Button } from "../atoms/button";

type Project = { id: string; generation: string; root: string; phase: string; outcome: string; savedVersion: string | null; captureAgeMs: number | null };
type Projection = { projects: Project[]; busy: boolean; error: string; available: boolean };
const empty: Projection = { projects: [], busy: false, error: "", available: false };
const send = (detail: Record<string, string>) => document.dispatchEvent(new CustomEvent("mesh:attachments-intent", { detail }));
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
          <Button variant="secondary" disabled={disabled || terminal || project.phase === "stopping"}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: "capture" })}>{t("Capture now")}</Button>
          <Button variant="secondary" disabled={disabled || project.phase === "stopping"}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: terminal ? "resume" : "stop" })}>{t(terminal ? "Resume capture" : "Stop capture")}</Button>
        </div>
      </article>;
    })}
    <p className="text-xs text-muted-foreground">{t("After reopening Mesh, attach the same project to resume capture. Saved history is retained.")}</p>
  </section>;
}

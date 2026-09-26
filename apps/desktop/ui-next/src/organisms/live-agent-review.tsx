import { useTranslation } from "../lib/localization";
import { useEffect, useMemo, useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import type {
  LiveReviewFilePreview,
  LiveReviewModel,
} from "../models/review-page";
import { liveReviewChangeProjection } from "../models/review-page";
import type { ReviewWorkbenchIntent } from "../models/review-workbench";

type LiveAgentReviewProps = {
  model: LiveReviewModel;
  preview: LiveReviewFilePreview | null;
  loading: boolean;
  error: string | null;
  errorPath: string | null;
  onIntent: (intent: ReviewWorkbenchIntent) => void;
};

const previewableKinds = new Set(["modified-file", "new-file"]);

export function LiveAgentReview({ model, preview, loading, error, errorPath, onIntent }: LiveAgentReviewProps) {
  const t = useTranslation();
  const [query, setQuery] = useState("");
  const changeProjection = useMemo(
    () => liveReviewChangeProjection(model.changes, query),
    [model.changes, query],
  );
  const visible = changeProjection.changes;
  const [selectedPath, setSelectedPath] = useState(() => (
    model.changes.find((change) => previewableKinds.has(change.kind))?.path ?? model.changes[0]?.path ?? ""
  ));
  useEffect(() => {
    if (model.changes.some((change) => change.path === selectedPath)) return;
    setSelectedPath(model.changes.find((change) => previewableKinds.has(change.kind))?.path ?? model.changes[0]?.path ?? "");
  }, [model.changes, selectedPath]);
  const selected = model.changes.find((change) => change.path === selectedPath) ?? null;
  const requestPreview = (path: string) => {
    setSelectedPath(path);
    const change = model.changes.find((candidate) => candidate.path === path);
    if (change && previewableKinds.has(change.kind)) onIntent({ type: "load-live-file", path });
  };
  const currentWorkspace = model.workspaces.find((workspace) => workspace.state === "current")?.path ?? model.workspaceRoot;

  return (
    <section className="grid gap-4" aria-label={t("Live agent work review")}>
      <header className="rounded-xl border border-amber-300/40 bg-amber-300/10 p-5">
        <div className="flex flex-wrap items-center gap-2">
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-amber-100">{t("Live agent work")}</p>
          <Badge tone="warning">{t("Mutable")}</Badge>
          <Badge tone="warning">{t("Unrecorded")}</Badge>
          <Badge tone={model.state === "error" ? "warning" : model.available ? "positive" : "neutral"}>
            {model.state === "scanning" ? t("Checking") : model.available ? t("Agent assigned") : t("No assigned agent")}
          </Badge>
        </div>
        <h2 className="mt-2 text-xl font-semibold">{t("Inspect work without finishing the handoff")}</h2>
        <p className="mt-2 text-sm leading-6 text-muted-foreground" role="status" aria-live="polite">{model.summary}</p>
        <p className="mt-2 text-xs leading-5 text-muted-foreground">
          {t("This surface is read-only. It cannot record, approve, export, save, or update the original folder. Finish agent handoff remains the authoritative complete scan and private save.")}</p>
        <label className="mt-4 grid max-w-xl gap-2 text-sm font-medium">
          {t("Workspace or assigned agent")}<select
            className="min-h-11 rounded-lg border border-border bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring"
            value={currentWorkspace}
            onChange={(event) => {
              const workspace = model.workspaces.find((candidate) => candidate.path === event.currentTarget.value);
              if (workspace?.canOpen) onIntent({ type: "switch-live-workspace", path: workspace.path });
            }}
          >
            {model.workspaces.map((workspace) => (
              <option key={workspace.path} value={workspace.path} disabled={!workspace.canOpen && workspace.state !== "current"}>
                {workspace.label} · {workspace.state.replaceAll("-", " ")}
              </option>
            ))}
          </select>
        </label>
      </header>

      {!model.available ? (
        <p className="rounded-xl border border-border bg-muted/20 p-5 text-sm text-muted-foreground">
          {t("Choose an assigned workspace above, or start an agent from Current. Saved Review remains available separately.")}</p>
      ) : (
        <div className="grid min-h-[30rem] overflow-hidden rounded-xl border border-border bg-background/40 lg:grid-cols-[20rem_minmax(0,1fr)]">
          <nav className="border-b border-border p-3 lg:border-b-0 lg:border-r" aria-label={t("Live changed files")}>
            <label className="grid gap-2 text-sm font-medium">
              <span className="sr-only">{t("Filter live changes")}</span>
              <input
                type="search" dir="auto"
                value={query}
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder={t("Filter live changes")}
                className="min-h-11 rounded-lg border border-border bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring"
              />
            </label>
            <p className="mt-2 text-xs text-muted-foreground">
              {changeProjection.truncated
                ? `${visible.length} of ${changeProjection.matched} visible changes shown`
                : `${changeProjection.matched} live changes`}
            </p>
            <div className="mt-3 grid max-h-[32rem] gap-1 overflow-auto">
              {visible.map((change) => (
                <button
                  key={`${change.kind}:${change.path}`}
                  type="button"
                  aria-pressed={selectedPath === change.path}
                  className={`min-h-11 rounded-lg border px-3 py-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${selectedPath === change.path ? "border-primary/50 bg-primary/10" : "border-transparent hover:bg-muted/60"}`}
                  onClick={() => requestPreview(change.path)}
                >
                  <span className="block truncate font-mono text-sm">{change.path}</span>
                  <span className="mt-1 block text-xs text-muted-foreground">{change.kind.replaceAll("-", " ")}</span>
                </button>
              ))}
              {changeProjection.truncated ? (
                <p className="rounded-lg border border-amber-400/25 bg-amber-400/5 p-3 text-xs leading-5 text-amber-100" role="status">
                  {t("Showing the first")}{" "}{visible.length} {t("visible live changes. Refine the search to inspect a narrower result set.")}</p>
              ) : null}
              {visible.length === 0 ? <p className="p-3 text-sm text-muted-foreground">{t("No live changes match this filter.")}</p> : null}
            </div>
          </nav>

          <section className="min-w-0 p-5" aria-label={t("Stable live file snapshot")} aria-busy={loading}>
            {selected ? (
              <>
                <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-4">
                  <div className="min-w-0">
                    <h3 className="truncate font-mono text-base font-semibold">{selected.path}</h3>
                    <p className="mt-1 text-xs text-muted-foreground">{t("Mutable working file · exact snapshot required")}</p>
                  </div>
                  <Button
                    variant="secondary"
                    disabled={loading || !previewableKinds.has(selected.kind)}
                    onClick={() => requestPreview(selected.path)}
                  >
                    {loading ? t("Reading…") : preview?.path === selected.path ? t("Retry snapshot") : t("Inspect stable snapshot")}
                  </Button>
                </div>
                {error && errorPath === selected.path ? (
                  <div className="mt-4 rounded-lg border border-amber-300/40 bg-amber-300/10 p-4" role="alert">
                    <h4 className="font-semibold">{t("Stable snapshot unavailable")}</h4>
                    <p className="mt-2 text-sm text-muted-foreground">{error}</p>
                    <p className="mt-2 text-xs text-muted-foreground">{t("Wait for the current write to finish, then retry. Nothing was recorded or saved.")}</p>
                  </div>
                ) : preview?.path === selected.path ? (
                  <div className="mt-4 grid gap-4">
                    <dl className="grid gap-3 rounded-lg border border-border bg-muted/20 p-4 text-sm sm:grid-cols-3">
                      <div><dt className="text-xs text-muted-foreground">{t("Bytes")}</dt><dd className="mt-1 font-mono">{preview.byteCount}</dd></div>
                      <div><dt className="text-xs text-muted-foreground">{t("Digest")}</dt><dd className="mt-1 truncate font-mono" title={preview.contentDigest}>{preview.contentDigest.slice(0, 16)}…</dd></div>
                      <div><dt className="text-xs text-muted-foreground">{t("Executable")}</dt><dd className="mt-1">{preview.executable ? t("Yes") : t("No")}</dd></div>
                    </dl>
                    {preview.imageDataUrl ? (
                      <figure className="overflow-hidden rounded-lg border border-border bg-background">
                        <img className="block max-h-[38rem] w-full bg-white object-contain" src={preview.imageDataUrl} alt={`Read-only live preview of ${preview.path}`} />
                        <figcaption className="border-t border-border p-3 text-xs text-muted-foreground">
                          {t("Inert")}{" "}{preview.previewKind === "artifact" ? t("document") : t("image")} {t("preview from one stable read pair. It is mutable and not recorded.")}</figcaption>
                      </figure>
                    ) : preview.text === null ? (
                      <p className="rounded-lg border border-border bg-muted/20 p-4 text-sm text-muted-foreground">
                        {preview.previewError || "The exact bytes were stable, but this binary or large file has no safe bounded preview."}
                      </p>
                    ) : (
                      <pre className="max-h-[34rem] overflow-auto rounded-lg border border-border bg-background p-4 font-mono text-xs leading-6" tabIndex={0}>{preview.text}</pre>
                    )}
                  </div>
                ) : selected.kind === "missing-file" ? (
                  <p className="mt-4 rounded-lg border border-border bg-muted/20 p-4 text-sm text-muted-foreground">{t("This saved path is currently missing. There are no live bytes to preview.")}</p>
                ) : selected.kind === "new-folder" ? (
                  <p className="mt-4 rounded-lg border border-border bg-muted/20 p-4 text-sm text-muted-foreground">{t("This is a new folder. Finish agent handoff will authoritatively inspect its complete descendants.")}</p>
                ) : selected.kind === "unsupported" ? (
                  <p className="mt-4 rounded-lg border border-border bg-muted/20 p-4 text-sm text-muted-foreground">{t("Mesh will not follow or preview this unsupported entry. Remove or replace it with a regular file, then retry.")}</p>
                ) : (
                  <p className="mt-4 text-sm text-muted-foreground">{t("Choose Inspect stable snapshot to read this changing file twice under exact agent custody.")}</p>
                )}
              </>
            ) : <p className="text-sm text-muted-foreground">{t("No live file changes are currently visible.")}</p>}
          </section>
        </div>
      )}
    </section>
  );
}

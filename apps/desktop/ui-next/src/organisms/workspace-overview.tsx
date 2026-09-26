import { useTranslation } from "../lib/localization";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { Card } from "../atoms/card";
import type { WorkspaceOverviewIntent, WorkspaceOverviewModel } from "../models/workspace-overview";

const stateTone = (state: WorkspaceOverviewModel["state"]): "positive" | "warning" | "neutral" => (
  state === "Approved" || state === "Available to team" || state === "Saved privately" ? "positive"
    : state === "Needs attention" ? "warning"
      : "neutral"
);

export function WorkspaceOverview({ model, onIntent }: {
  model: WorkspaceOverviewModel;
  onIntent: (intent: WorkspaceOverviewIntent) => void;
}) {
  const t = useTranslation();
  return (
    <Card aria-label={t("Workspace overview")} className="overflow-hidden">
      <div className="grid gap-5 p-5 lg:grid-cols-[minmax(0,1fr)_minmax(18rem,0.72fr)] lg:p-6">
        <section className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">{t("Current workspace")}</p>
            <Badge tone={stateTone(model.state)}>{model.state}</Badge>
          </div>
          <h2 className="mt-2 truncate text-2xl font-semibold tracking-tight">{model.workspaceName}</h2>
          <p className="mt-2 truncate text-sm text-muted-foreground" title={model.workingFolder}>{model.workingFolder}</p>
          <dl className="mt-5 grid gap-3 text-sm sm:grid-cols-2 xl:grid-cols-4">
            <Fact label={t("Private history")} value={model.privateVersion} />
            <Fact label={t("Shared version")} value={model.sharedVersion} />
            <Fact label={t("Saved points")} value={String(model.savedVersionCount)} />
            <Fact label={t("Folder changes")} value={String(model.nativeChangeCount)} />
          </dl>
          <p className="mt-4 text-xs text-muted-foreground">{model.recordSummary}</p>
          <div className="mt-5 flex flex-wrap gap-2">
            <Button variant="secondary" disabled={!model.canOpenFolder} onClick={() => onIntent({ type: "open-folder" })}>{t("Open folder")}</Button>
            <Button variant="secondary" disabled={!model.canFindChanges} onClick={() => onIntent({ type: "find-changes" })}>{t("Find changes")}</Button>
            <Button variant="quiet" disabled={!model.canOpenReview} onClick={() => onIntent({ type: "open-review" })}>{t("Review work")}</Button>
            <Button variant="quiet" disabled={!model.canCopyDiagnostics} onClick={() => onIntent({ type: "copy-diagnostics" })}>{t("Copy safe diagnostics")}</Button>
            {model.canReturnWorkspace ? (
              <Button variant="secondary" onClick={() => onIntent({ type: "return-workspace" })}>{t("Return to previous workspace")}</Button>
            ) : null}
          </div>
        </section>
        <aside className="rounded-xl border border-border bg-muted/25 p-4" aria-label={t("Recommended next action")}>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t("Recommended next")}</p>
          <h3 className="mt-2 text-lg font-semibold">{t(model.nextActionTitle)}</h3>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">{t(model.nextActionDescription)}</p>
          <div className="mt-5 grid gap-2">
            <Button variant="primary" className="w-full" disabled={model.nextActionDisabled} onClick={() => onIntent({ type: "recommended" })}>
              {t(model.nextActionLabel)}
            </Button>
            {model.canOpenAnotherVersion ? (
              <Button variant="secondary" className="w-full" onClick={() => onIntent({ type: "open-another-version" })}>
                {t("Open another version")}</Button>
            ) : null}
          </div>
        </aside>
      </div>
    </Card>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  const t = useTranslation();
  return (
    <div className="min-w-0 rounded-lg border border-border bg-background/40 p-3">
      <dt className="text-xs text-muted-foreground">{t(label)}</dt>
      <dd className="mt-1 truncate font-medium" title={value}>{value}</dd>
    </div>
  );
}

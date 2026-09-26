import { useTranslation } from "../lib/localization";
import { Badge } from "../atoms/badge";
import type { RestorePreview } from "../models/workspace-restore";

export function RestorePreviewCard({ preview, canApply }: Readonly<{
  preview: RestorePreview;
  canApply: boolean;
}>) {
  const t = useTranslation();
  return (
    <section
      aria-labelledby="restore-next-preview-title"
      role="status"
      aria-live="polite"
      aria-atomic="true"
      className="rounded-xl border border-border bg-background/50 p-4"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={canApply ? "changed" : "warning"}>{canApply ? t("Ready to restore") : t("Preview verified")}</Badge>
        <Badge tone="neutral">{preview.format}</Badge>
      </div>
      <h3 id="restore-next-preview-title" className="mt-3 break-all text-base font-semibold">{preview.filePath}</h3>
      <dl className="mt-4 grid gap-3 text-sm sm:grid-cols-2">
        <div className="min-w-0 rounded-lg border border-border p-3">
          <dt className="text-xs text-muted-foreground">{t("Current saved version")}</dt>
          <dd className="mt-1 truncate font-mono" title={preview.currentVersion}>{preview.currentVersion}</dd>
        </div>
        <div className="min-w-0 rounded-lg border border-border p-3">
          <dt className="text-xs text-muted-foreground">{t("Version to restore")}</dt>
          <dd className="mt-1 truncate font-mono" title={preview.targetVersion}>{preview.targetVersion}</dd>
        </div>
      </dl>
      <div className="mt-4 space-y-2 text-sm leading-6 text-muted-foreground">
        <p><strong className="text-foreground">{t("What changes:")}</strong> {preview.change}</p>
        <p><strong className="text-foreground">{t("History:")}</strong> {preview.historyNote}</p>
        <p><strong className="text-foreground">{t("Undo:")}</strong> {preview.undoNote}</p>
        {!canApply ? (
          <p><strong className="text-foreground">{t("Restore is currently unavailable.")}</strong> {t("Resolve the active workspace condition before applying this verified preview.")}</p>
        ) : null}
      </div>
    </section>
  );
}

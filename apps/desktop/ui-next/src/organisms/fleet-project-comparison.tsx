import { useTranslation } from "../lib/localization";
import { Button } from "../atoms/button";
import { TextComparison } from "./workspace-files-changes";
import type { InputSide } from "./fleet-input-comparison";

export type CandidateSelector = { project: string; request: string; expected_main: string | null };
type Change = { path: string; effect: string; before: InputSide | null; after: InputSide | null };
type Page = { review: string; baseIsCurrent: boolean; observedMain: { head: string } | null; context: { base_head: string | null; target_version: string }; total: number; changes: Change[]; nextAfter: string | null };
export type ProjectComparison = { loading: boolean; error: string; page: Page | null; fixed?: Page; file: Change | null };
const send = (detail: Record<string, string>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));
function Side({ side, label }: { side: InputSide | null; label: string }) {
  const t = useTranslation();
  return <section className="min-w-0"><h6 className="font-semibold">{t(label)}</h6>
    {!side ? <p>{t("Absent in this version")}</p> : <><p>{side.kind === "folder" ? t("Folder") : <>{side.bytes} {t("bytes")} · {t(side.executable ? "Executable" : "Not executable")}</>}</p>
      {side.state === "text" && <pre dir="ltr" className="max-h-72 overflow-auto whitespace-pre-wrap rounded bg-muted p-2">{side.text}</pre>}
      {side.state === "too-large" && <p>{t("File exceeds the 256 KiB text preview limit.")}</p>}
      {side.state === "binary-or-unsafe-text" && <p>{t("Binary or unsafe text content; text preview unavailable.")}</p>}
      {side.digest && <details className="break-all"><summary>{t("Content identity")}</summary><bdi dir="ltr">{side.digest}</bdi></details>}</>}
  </section>;
}
export function FleetProjectComparison({ pin, pending, comparison, enabled, project }: { pin: string; pending?: CandidateSelector | null; comparison?: ProjectComparison; enabled: boolean; project?: string | null }) {
  const t = useTranslation();
  const busy = Boolean(comparison?.loading), page = comparison?.page, fixed = comparison?.fixed ?? page, file = comparison?.file;
  const text = file && [file.before, file.after].every(side => side === null || side.state === "text") && (file.before?.kind === "file" || file.after?.kind === "file");
  return <section className="grid min-w-0 gap-3 rounded border border-border p-3" aria-label={t("Fixed project comparison")}>
    <h5 className="font-semibold">{t("Compare this result with project main")}</h5>
    <p className="text-xs text-muted-foreground">{t("Shows the complete proposed project, including inherited work. Preparation fixes the main version and preserves the request before copying saved content.")}</p>
    {!pending && <Button variant="secondary" disabled={!enabled || busy || !project} onClick={() => send({ type: "candidate-prepare", pin })}>{t("Prepare comparison with current main")}</Button>}
    {pending && <><p className="break-all text-xs">{t("Fixed main")}: {pending.expected_main ? <bdi dir="ltr">{pending.expected_main}</bdi> : t("Initial empty main")}</p>
      <details className="break-all text-xs"><summary>{t("Saved preparation inputs")}</summary><p>{t("Project")}: <bdi dir="ltr">{pending.project}</bdi></p><p>{t("Request")}: <bdi dir="ltr">{pending.request}</bdi></p></details>
      <Button variant="secondary" disabled={!enabled || busy} onClick={() => send({ type: "candidate-read", pin })}>{t("Read fixed comparison from first page")}</Button>
      {!page && <Button variant="secondary" disabled={!enabled || busy} onClick={() => send({ type: "candidate-prepare", pin })}>{t("Retry preparing these exact inputs")}</Button>}</>}
    {busy && <p role="status">{t("Verifying the fixed project comparison…")}</p>}
    {comparison?.error && <><p role="alert">{t(comparison.error)} {t("Any content below is the previously verified result.")}</p><Button variant="secondary" disabled={!enabled || busy} onClick={() => send({ type: "candidate-retry", pin })}>{t("Retry the last comparison request")}</Button></>}
    {fixed && <><p role="status">{t(fixed.baseIsCurrent ? "Main matched this base when last checked." : "Main has advanced. This comparison remains pinned to its original base.")}</p>
      <details className="break-all text-xs"><summary>{t("Exact comparison identity")}</summary><p>{t("Comparison")}: <bdi dir="ltr">{fixed.review}</bdi></p><p>{t("Saved result")}: <bdi dir="ltr">{fixed.context.target_version}</bdi></p><p>{t("Last observed main")}: {fixed.observedMain ? <bdi dir="ltr">{fixed.observedMain.head}</bdi> : t("Initial empty main")}</p></details></>}
    {page && <><p>{page.total} {t("changed paths")} · {page.changes.length} {t("on this page")}</p>
      {page.total === 0 && <p>{t("No content, path or executable-mode changes against this main version.")}</p>}
      <ul className="grid max-h-64 gap-2 overflow-auto text-sm">{page.changes.map(change => <li key={change.path}><button className="break-all text-left underline" disabled={!enabled || busy} aria-pressed={file?.path === change.path} onClick={() => send({ type: "candidate-file", pin, path: change.path })}>{t(change.effect === "added" ? "Added" : change.effect === "removed" ? "Removed" : "Content or metadata changed")} · <bdi dir="ltr">{change.path}</bdi></button></li>)}</ul>
      {page.nextAfter && <Button variant="secondary" disabled={!enabled || busy} onClick={() => send({ type: "candidate-page", pin, after: page.nextAfter! })}>{t("Next changed paths")}</Button>}</>}
    {file && <div className="grid min-w-0 gap-3"><p className="break-all font-semibold">{t("Selected")}: <bdi dir="ltr">{file.path}</bdi></p>
      {text && <TextComparison before={file.before?.text ?? ""} after={file.after?.text ?? ""} split context="saved" />}
      <div className="grid gap-3 text-xs"><Side side={file.before} label="Fixed project main" /><Side side={file.after} label="Proposed saved result" /></div></div>}
    <p className="text-xs text-muted-foreground">{t("Preparation and inspection do not approve changes, advance main, or replace working files. Reopening only reads saved content and never restarts workers.")}</p>
  </section>;
}

import { useState } from "react";
import { useTranslation } from "../lib/localization";
import { Button } from "../atoms/button";
import { TextComparison } from "./workspace-files-changes";

export type InputSide = { path: string; kind: "file" | "folder"; digest: string | null; bytes: number | null; executable: boolean | null; state: "text" | "folder" | "not-requested" | "too-large" | "binary-or-unsafe-text"; text: string | null };
export type InputChange = { object: string; effect: string; before: InputSide | null; after: InputSide | null };
export type InputComparison = { loading: boolean; error: string; file: InputChange | null; page: { base: string; target: string; total: number; after: string | null; nextAfter: string | null; changes: InputChange[] } | null };
const send = (detail: Record<string, string>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));
const path = (change: InputChange) => change.before && change.after && change.before.path !== change.after.path ? `${change.before.path} → ${change.after.path}` : change.after?.path ?? change.before?.path;
function Side({ label, side }: { label: string; side: InputSide | null }) {
  const t = useTranslation();
  return <section className="min-w-0" aria-label={t(label)}><h6 className="font-semibold">{t(label)}</h6>
    {!side ? <p>{t("Absent in this version")}</p> : <>
      <p className="break-all"><bdi dir="ltr">{side.path}</bdi></p>
      <p>{side.kind === "folder" ? t("Folder") : <>{side.bytes} {t("bytes")} · {t(side.executable ? "Executable" : "Not executable")}</>}</p>
      {side.digest && <details className="break-all"><summary>{t("Content identity")}</summary><bdi dir="ltr">{side.digest}</bdi></details>}
      {side.state === "text" ? <pre dir="ltr" className="max-h-72 overflow-auto whitespace-pre-wrap rounded bg-muted p-2">{side.text}</pre>
        : side.state === "too-large" ? <p>{t("File exceeds the 256 KiB text preview limit.")}</p>
        : side.state === "binary-or-unsafe-text" ? <p>{t("Binary or unsafe text content; text preview unavailable.")}</p> : null}
    </>}
  </section>;
}
export function FleetInputComparison({ pin, input }: { pin: string; input?: InputComparison }) {
  const t = useTranslation();
  const [split, setSplit] = useState(false);
  const file = input?.file;
  const textAvailable = file && [file.before, file.after].every(side => side === null || side.state === "text") && (file.before?.kind === "file" || file.after?.kind === "file");
  return <section className="grid min-w-0 gap-3 rounded border border-border p-3" aria-label={t("Changes since lane start")}>
    <h5 className="font-semibold">{t("Changes since this lane started")}</h5>
    <p className="text-xs text-muted-foreground">{t("Compares the verified starting version with this pinned saved result. Current working files do not enter this comparison.")}</p>
    <Button variant="secondary" disabled={input?.loading} onClick={() => send({ type: "input-review", pin })}>{t(input?.page ? "First page of changes" : "Compare with starting version")}</Button>
    {input?.loading && <p role="status">{t("Reading the exact starting-version comparison\u2026 The previous selection remains below.")}</p>}
    {input?.error && <><p role="alert">{t(input.error)}</p><Button variant="secondary" disabled={input.loading} onClick={() => send({ type: "retry-input", pin })}>{t("Retry this comparison request")}</Button></>}
    {input?.page && <>
      <p>{input.page.total} {t("changed objects")} · {input.page.changes.length} {t("on this page")}</p>
      <details className="break-all text-xs"><summary>{t("Exact comparison versions")}</summary><p>{t("Local starting version")}: <bdi dir="ltr">{input.page.base}</bdi></p><p>{t("Pinned result")}: <bdi dir="ltr">{input.page.target}</bdi></p></details>
      {input.page.total === 0 && <p>{t("No path, content or executable-mode changes since this lane started.")}</p>}
      <ul className="grid max-h-64 gap-2 overflow-auto text-sm">{input.page.changes.map(change => <li key={change.object}><button className="break-all text-left underline" disabled={input.loading} aria-pressed={file?.object === change.object} onClick={() => send({ type: "input-file", pin, object: change.object })}>{t(change.effect === "added" ? "Added" : change.effect === "removed" ? "Removed" : change.effect === "moved-or-modified" ? "Moved; content may also differ" : "Content or metadata changed")} · <bdi dir="ltr">{path(change)}</bdi></button></li>)}</ul>
      {input.page.nextAfter && <Button variant="secondary" disabled={input.loading} onClick={() => send({ type: "input-page", pin, after: input.page!.nextAfter! })}>{t("Next changed objects")}</Button>}
    </>}
    {file && <div className="grid min-w-0 gap-3"><p className="break-all font-semibold">{t("Selected")}: <bdi dir="ltr">{path(file)}</bdi></p>
      {textAvailable && <><Button variant="quiet" onClick={() => setSplit(value => !value)}>{t(split ? "Use inline comparison" : "Use side-by-side comparison")}</Button><TextComparison before={file.before?.text ?? ""} after={file.after?.text ?? ""} split={split} context="saved" /></>}
      <div className="grid gap-3 text-xs"><Side label="Lane starting version" side={file.before} /><Side label="Pinned saved result" side={file.after} /></div>
    </div>}
    <p className="text-xs text-muted-foreground">{t("Read-only comparison. It does not approve or apply changes to main.")}</p>
  </section>;
}

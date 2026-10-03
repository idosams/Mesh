import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
export type RemoteResultPage = { available: boolean; count: number; revision: string | null; hasMore: boolean; before: number; after: number | null; entries: { offer: string; checkpoint: string; version: string; review: string; manifest: string }[] };
const send = (type: string, offer?: string) => document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent", { detail: { type, offer } }));
export function RemoteResults({ page, disabled }: { page: RemoteResultPage; disabled: boolean }) {
  const t = useTranslation();
  if (!page.available) return <p role="status">{t("Saved result history is unavailable for this assignment.")}</p>;
  return <section className="grid gap-3 text-sm" aria-label={t("Saved remote results")}>
    <p role="status">{t("Saved result offers in this page")}: {page.count}</p>
    <p>{t("Listing a result does not download or accept it. Download creates a private copy for review.")}</p>
    <ol className="grid gap-2" start={page.before + 1}>{page.entries.map((entry, index) => <li className="rounded border p-3" key={entry.offer}>
      <p>{t("Saved result")} {page.before + index + 1}</p>
      <Button disabled={disabled} onClick={() => send("download-result", entry.offer)}>{t("Download for review")}</Button>
      <details><summary className="cursor-pointer">{t("Result identities")}</summary><dl className="mt-2 grid gap-1 break-all">{([["Checkpoint", entry.checkpoint], ["Saved version", entry.version], ["Review identity", entry.review], ["Result manifest", entry.manifest], ["Result offer", entry.offer]]).map(([label, value]) => <div key={label}><dt className="font-medium">{t(label)}</dt><dd><bdi dir="ltr">{value}</bdi></dd></div>)}</dl></details>
    </li>)}</ol>
    <div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={disabled || page.before === 0} onClick={() => send("results-previous")}>{t("Previous results")}</Button><Button variant="secondary" disabled={disabled || !page.hasMore} onClick={() => send("results-next")}>{t("Next results")}</Button></div>
    {page.hasMore && <p>{t("More saved results exist beyond this page.")}</p>}
  </section>;
}

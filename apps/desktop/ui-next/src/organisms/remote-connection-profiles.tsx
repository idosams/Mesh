import { useState } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
export type RemoteProfiles = { revision: string; entries: { id: string; label: string; host: string; worker: string; objective: string; lane: string; run: string }[] };
const send = (detail: Record<string, unknown>) => document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent", { detail }));
export function RemoteConnectionProfiles({ profiles, selected, disabled }: { profiles: RemoteProfiles | null; selected: boolean; disabled: boolean }) {
  const t = useTranslation();
  const [label, setLabel] = useState("");
  return <section className="grid gap-3 rounded border p-3" aria-label={t("Saved worker connections")}>
    <h3 className="font-medium">{t("Saved worker connections")}</h3>
    <p className="text-sm">{t("Open saved settings to use or edit them. Mesh checks the original files and identities again. Opening settings does not contact the worker.")}</p>
    <div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={disabled} onClick={() => send({ type: "profiles-list" })}>{t("Load saved connections")}</Button><Button variant="secondary" disabled={disabled} onClick={() => send({ type: "profiles-recover" })}>{t("Recover interrupted settings save")}</Button></div>
    {profiles && <><p className="text-sm" role="status">{t("Saved connections")}: {profiles.entries.length} / 16</p>
      <ul className="grid gap-2">{profiles.entries.map(entry => <li key={entry.id} className="grid gap-2 rounded border p-2"><p><bdi>{entry.label}</bdi> · <bdi dir="ltr">{entry.host}</bdi></p><p className="break-all text-xs"><bdi dir="ltr">{entry.objective} / {entry.lane} / {entry.run}</bdi></p><div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={disabled} onClick={() => send({ type: "profile-open", profile: entry.id })}>{t("Open saved settings")}</Button><Button variant="secondary" disabled={disabled} onClick={() => send({ type: "profile-remove", profile: entry.id })}>{t("Remove saved settings")}</Button></div></li>)}</ul>
      <label className="grid gap-1 text-sm">{t("Connection name")}<input className="min-h-11 rounded border bg-background p-2" maxLength={128} disabled={disabled} value={label} onChange={event => setLabel(event.target.value)} /></label>
      <Button disabled={disabled || !selected || !label.trim()} onClick={() => send({ type: "profile-save", label })}>{t("Save current connection")}</Button>
    </>}
    <p className="text-xs">{t("After opening and editing saved settings, save the current connection to replace that entry. Removing settings keeps the active selection, credentials and work history.")}</p>
  </section>;
}

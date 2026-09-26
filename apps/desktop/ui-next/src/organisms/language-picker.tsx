import { useSyncExternalStore } from "react";
import { getLocale, setLocale, subscribeLocale } from "../lib/localization";
export function LanguagePicker() {
  const locale = useSyncExternalStore(subscribeLocale, getLocale, getLocale);
  return <label className="flex items-center gap-2 text-xs" dir="auto">
    <span>{locale === "he" ? "שפה" : "Language"}</span>
    <select data-mesh-proof="language-picker" aria-label={locale === "he" ? "שפת הממשק" : "Interface language"} value={locale}
      className="min-h-11 rounded-lg border border-border bg-background px-3 text-foreground"
      onChange={(event) => setLocale(event.currentTarget.value === "he" ? "he" : "en")}>
      <option value="en" lang="en">English</option><option value="he" lang="he">עברית</option>
    </select>
  </label>;
}

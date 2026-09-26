import { useSyncExternalStore } from "react";
import { translateSafety } from "./hebrew-safety";
import { hebrew } from "./hebrew";
export type Locale = "en" | "he";
export const LOCALE_KEY = "mesh.ui.locale.v1";
export function resolveLocale(saved: string | null, language = "en"): Locale {
  return saved === "he" || saved === "en" ? saved : /^he(?:-|$)/i.test(language) ? "he" : "en";
}
export function translate(locale: Locale, text: string | null | undefined): string {
  if (text == null) return "";
  return locale === "he" ? hebrew[text] ?? translateSafety(text) ?? text : text;
}
let locale: Locale = "en";
try { locale = resolveLocale(globalThis.localStorage?.getItem(LOCALE_KEY) ?? null); } catch { /* Storage may be denied; English remains usable. */ }
const listeners = new Set<() => void>();
export function getLocale(): Locale { return locale; }
export function subscribeLocale(listener: () => void): () => void { listeners.add(listener); return () => listeners.delete(listener); }
function applyDirection() {
  if (typeof document === "undefined" || !document.documentElement) return;
  document.documentElement.lang = locale;
  document.documentElement.dir = locale === "he" ? "rtl" : "ltr";
}
export function setLocale(next: Locale): void {
  if (next !== "en" && next !== "he") return;
  locale = next;
  try { globalThis.localStorage?.setItem(LOCALE_KEY, next); } catch { /* Session preference still works without persistence. */ }
  applyDirection();
  listeners.forEach((listener) => listener());
}
export function useTranslation() {
  const selected = useSyncExternalStore(subscribeLocale, getLocale, getLocale);
  return (text: string | null | undefined) => translate(selected, text);
}
applyDirection();

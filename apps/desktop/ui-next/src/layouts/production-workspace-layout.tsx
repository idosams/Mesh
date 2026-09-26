import { useTranslation } from "../lib/localization";
import { useRef, type ReactNode } from "react";

export function activateWorkspaceSkipLink(
  event: Readonly<{ preventDefault: () => void }>,
  main: HTMLElement | null,
): boolean {
  event.preventDefault();
  if (!main) return false;
  main.focus({ preventScroll: true });
  return true;
}

export function ProductionWorkspaceLayout({ header, navigation, notice, children, buildIdentity }: Readonly<{
  header: ReactNode;
  navigation: ReactNode;
  notice: ReactNode;
  children: ReactNode;
  buildIdentity: Readonly<{ label: string; title: string }>;
}>) {
  const t = useTranslation();
  const mainRef = useRef<HTMLElement | null>(null);
  return (
    <div id="mesh-react-page-content" className="min-h-screen bg-background text-foreground">
      <a
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-lg focus:bg-primary focus:px-4 focus:py-3 focus:font-bold focus:text-primary-foreground"
        href="#mesh-react-main"
        onClick={(event) => activateWorkspaceSkipLink(event, mainRef.current)}
      >
        {t("Skip to workspace")}</a>
      <header className="border-b border-border bg-background/95">
        <div className="mx-auto max-w-[100rem] px-4 sm:px-6 lg:px-8">{header}</div>
      </header>
      <div className="mx-auto max-w-[100rem] px-4 pt-3 sm:px-6 lg:px-8">{navigation}</div>
      <div className="mx-auto max-w-[100rem] px-4 sm:px-6 lg:px-8">{notice}</div>
      <main ref={mainRef} id="mesh-react-main" tabIndex={-1} className="mx-auto max-w-[100rem] px-4 py-4 sm:px-6 lg:px-8">
        {children}
      </main>
      <footer className="mx-auto max-w-[100rem] px-4 py-6 text-xs text-muted-foreground sm:px-6 lg:px-8">
        <span title={buildIdentity.title}>{buildIdentity.label}</span> {t("· Local only · No network listener")}</footer>
    </div>
  );
}

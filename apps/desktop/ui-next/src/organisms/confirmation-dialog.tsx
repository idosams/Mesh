import { useTranslation } from "../lib/localization";
import { useId, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { Button } from "../atoms/button";
import {
  confirmationKeyboardAction,
  type ConfirmationIntent,
  type ConfirmationModel,
} from "../models/confirmation";

type ActiveElementRoot = Pick<Document | ShadowRoot, "activeElement">;
type PageRoot = Pick<Document, "getElementById">;

export function isolateVisiblePage(root: PageRoot): () => void {
  const candidate = root
    .getElementById("mesh-app-next")
    ?.shadowRoot
    ?.getElementById("mesh-react-page-content");
  const page = candidate instanceof HTMLElement ? candidate : null;
  const wasInert = page?.inert ?? false;
  if (page) page.inert = true;
  return () => {
    if (page) page.inert = wasInert;
  };
}

export function deepestActiveElement(root: ActiveElementRoot): HTMLElement | null {
  let active = root.activeElement;
  while (active instanceof HTMLElement) {
    const nested = active.shadowRoot?.activeElement;
    if (!(nested instanceof HTMLElement)) return active;
    active = nested;
  }
  return null;
}

export function restoreDialogFocus(
  root: ActiveElementRoot,
  previousFocus: HTMLElement | null,
  dialog: HTMLElement | null,
): boolean {
  const currentFocus = deepestActiveElement(root);
  if (!previousFocus?.isConnected || !dialog || !currentFocus || !dialog.contains(currentFocus)) {
    return false;
  }
  previousFocus.focus({ preventScroll: true });
  return true;
}

export function ConfirmationDialog({ model, onIntent }: {
  model: ConfirmationModel;
  onIntent: (intent: ConfirmationIntent) => void;
}) {
  const t = useTranslation();
  const titleId = useId();
  const descriptionId = useId();
  const dialog = useRef<HTMLElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);

  useLayoutEffect(() => {
    const previousFocus = deepestActiveElement(document);
    const mountedDialog = dialog.current;
    const restorePageIsolation = isolateVisiblePage(document);
    cancelButton.current?.focus({ preventScroll: true });
    return () => {
      restorePageIsolation();
      restoreDialogFocus(document, previousFocus, mountedDialog);
    };
  }, []);

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const action = confirmationKeyboardAction(
      event.key,
      event.shiftKey,
      event.target === cancelButton.current,
      event.target === confirmButton.current,
    );
    if (action === "cancel") {
      event.preventDefault();
      event.stopPropagation();
      onIntent({ type: "cancel" });
      return;
    }
    const first = cancelButton.current;
    const last = confirmButton.current;
    if (!first || !last) return;
    if (action === "focus-confirm") {
      event.preventDefault();
      last.focus();
    } else if (action === "focus-cancel") {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/75 p-4 sm:p-8"
      data-mesh-proof="confirmation-backdrop"
      onKeyDown={handleKeyDown}
    >
      <section
        ref={dialog}
        role={model.tone === "destructive" ? "alertdialog" : "dialog"}
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        className="grid max-h-[calc(100vh-2rem)] w-full max-w-xl gap-5 overflow-y-auto rounded-xl border border-border bg-card p-5 text-card-foreground shadow-2xl sm:p-6"
      >
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">{t("Confirm exact action")}</p>
          <h2 id={titleId} className="mt-2 text-xl font-semibold tracking-tight">{t(model.title)}</h2>
          <p id={descriptionId} className="mt-3 whitespace-pre-wrap break-words text-sm leading-6 text-muted-foreground">{t(model.description)}</p>
        </div>
        <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
          <Button ref={cancelButton} variant="secondary" onClick={() => onIntent({ type: "cancel" })}>
            {t(model.cancelLabel)}
          </Button>
          <Button
            ref={confirmButton}
            data-mesh-proof="confirmation-accept"
            variant={model.tone === "destructive" ? "danger" : "primary"}
            onClick={() => onIntent({ type: "confirm" })}
          >
            {t(model.confirmLabel)}
          </Button>
        </div>
      </section>
    </div>
  );
}

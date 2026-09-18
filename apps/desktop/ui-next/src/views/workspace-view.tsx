import { useEffect, useRef, useState, type ReactNode } from "react";

export function WorkspaceView({ active, label, children }: Readonly<{
  active: boolean;
  label: string;
  children: ReactNode;
}>) {
  return (
    <section
      aria-label={label}
      className="min-w-0"
      hidden={!active}
      data-mesh-page-active={active ? "true" : "false"}
    >
      {children}
    </section>
  );
}

export function IslandSlot({ name, label, failed = false, chrome = "card" }: Readonly<{
  name: string;
  label: string;
  failed?: boolean;
  chrome?: "card" | "workbench";
}>) {
  const slotRef = useRef<HTMLSlotElement>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const slot = slotRef.current;
    if (!slot) return;
    let observer: MutationObserver | null = null;
    const inspect = () => {
      observer?.disconnect();
      const assigned = slot.assignedElements()[0] as HTMLElement | undefined;
      setReady(Boolean(assigned && !assigned.classList.contains("hidden")));
      if (assigned) {
        observer = new MutationObserver(inspect);
        observer.observe(assigned, { attributes: true, attributeFilter: ["class"] });
      }
    };
    slot.addEventListener("slotchange", inspect);
    inspect();
    return () => {
      slot.removeEventListener("slotchange", inspect);
      observer?.disconnect();
    };
  }, [name]);
  const frame = chrome === "workbench"
    ? "min-w-0 overflow-hidden rounded-xl border border-border bg-card shadow-2xl shadow-black/15"
    : "min-w-0 rounded-2xl border border-border bg-card p-4 shadow-2xl sm:p-6";
  return (
    <div className={frame} data-mesh-slot-chrome={chrome}>
      <slot ref={slotRef} name={name} />
      {failed && !ready ? (
        <div className="space-y-3 p-4 sm:p-6" data-mesh-slot-failure={name} role="alert">
          <p className="m-0 text-sm text-foreground">This page could not finish rendering safely.</p>
          <button
            type="button"
            className="min-h-11 rounded-lg border border-border px-4 text-sm font-semibold hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => window.location.reload()}
          >Reload Mesh</button>
        </div>
      ) : (
        <p className="m-0 p-4 text-sm text-muted-foreground sm:p-6" data-mesh-slot-status={name} hidden={ready}>
          {label}
        </p>
      )}
    </div>
  );
}

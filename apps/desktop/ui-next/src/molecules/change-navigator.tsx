import { Badge } from "../atoms/badge";
import type { KeyboardEvent } from "react";
import { rovingSelectionIndex } from "../models/roving-selection";
import type { ReviewChange } from "../models/review-workbench";

type ChangeNavigatorProps = {
  changes: readonly ReviewChange[];
  selectedChangeId: string;
  onSelect: (changeId: string) => void;
};

export function ChangeNavigator({ changes, selectedChangeId, onSelect }: ChangeNavigatorProps) {
  const moveSelection = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = rovingSelectionIndex(changes.length, index, event.key, "vertical-clamp");
    if (next === null) return;
    event.preventDefault();
    const buttons = event.currentTarget.parentElement
      ?.querySelectorAll<HTMLButtonElement>("[data-change-option]");
    buttons?.[next]?.focus();
    onSelect(changes[next].id);
  };
  return (
    <nav aria-label="Changed files" className="border-b border-border bg-card/45 xl:border-b-0 xl:border-r">
      <div className="border-b border-border px-4 py-3">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">
          Changes · {changes.length}
        </p>
      </div>
      <div className="grid max-h-64 gap-1 overflow-y-auto p-2 xl:max-h-[40rem]">
        {changes.map((change) => {
          const selected = change.id === selectedChangeId;
          return (
            <button
              type="button"
              key={change.id}
              data-change-option
              aria-pressed={selected}
              tabIndex={selected ? 0 : -1}
              className={selected
                ? "rounded-lg border border-sky-300/35 bg-sky-300/10 px-3 py-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                : "rounded-lg border border-transparent px-3 py-3 text-left hover:bg-muted/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"}
              onClick={() => onSelect(change.id)}
              onKeyDown={(event) => moveSelection(event, changes.indexOf(change))}
            >
              <span className="block truncate text-sm font-semibold">{change.path}</span>
              <span className="mt-2 flex items-center justify-between gap-2">
                <Badge tone="changed">{change.kindLabel}</Badge>
                <span className="text-xs text-muted-foreground">{change.impact}</span>
              </span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}

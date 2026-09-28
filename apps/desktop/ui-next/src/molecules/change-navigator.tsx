import { useTranslation } from "../lib/localization";
import { useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { rovingSelectionIndex } from "../models/roving-selection";
import type { ReviewChange } from "../models/review-workbench";

type ChangeNavigatorProps = {
  changes: readonly ReviewChange[];
  selectedChangeId: string;
  onSelect: (changeId: string) => void;
};

type StatusFilter = "all" | ReviewChange["status"];
type KindFilter = "all" | ReviewChange["kind"];

const statusLabels: Readonly<Record<ReviewChange["status"], string>> = Object.freeze({
  added: "Added",
  modified: "Modified",
  moved: "Moved",
  deleted: "Deleted",
  unsupported: "Unsupported",
});

function changeFolder(change: ReviewChange): string {
  if (change.folder) return change.folder;
  const path = change.path.includes(" → ") ? change.path.split(" → ").at(-1) as string : change.path;
  return path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "Workspace root";
}

function changeStatus(change: ReviewChange): ReviewChange["status"] {
  return change.status ?? (change.kind === "file" ? "unsupported" : "modified");
}

export function ChangeNavigator({ changes, selectedChangeId, onSelect }: ChangeNavigatorProps) {
  const t = useTranslation();
  const instanceId = useId();
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [kind, setKind] = useState<KindFilter>("all");
  const navigatorRef = useRef<HTMLElement>(null);
  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return changes.filter((change) => (
      (!normalized || change.path.toLocaleLowerCase().includes(normalized))
      && (status === "all" || changeStatus(change) === status)
      && (kind === "all" || change.kind === kind)
    ));
  }, [changes, kind, query, status]);
  const groups = useMemo(() => {
    const grouped = new Map<string, ReviewChange[]>();
    for (const change of filtered) {
      const folder = changeFolder(change);
      const group = grouped.get(folder) ?? [];
      group.push(change);
      grouped.set(folder, group);
    }
    return [...grouped.entries()].sort(([left], [right]) => left.localeCompare(right));
  }, [filtered]);
  const selectedIndex = filtered.findIndex((change) => change.id === selectedChangeId);
  const activeIndex = selectedIndex < 0 ? 0 : selectedIndex;
  const selectAt = (index: number) => {
    const change = filtered[index];
    if (!change) return;
    onSelect(change.id);
    requestAnimationFrame(() => navigatorRef.current
      ?.querySelector<HTMLButtonElement>(`[data-change-option="${change.id}"]`)
      ?.focus());
  };
  const moveSelection = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = rovingSelectionIndex(filtered.length, index, event.key, "vertical-clamp");
    if (next === null) return;
    event.preventDefault();
    selectAt(next);
  };
  const kinds = [...new Set(changes.map((change) => change.kind))].sort();

  return (
    <nav ref={navigatorRef} aria-label={t("Changed files")} className="border-b border-border bg-card/45 xl:border-b-0 xl:border-r">
      <div className="grid gap-3 border-b border-border p-4">
        <div className="flex items-center justify-between gap-2">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t("Changed files")}</p>
            <p className="mt-1 text-xs text-muted-foreground">{filtered.length} {t("of")}{" "}{changes.length}</p>
          </div>
          <div className="flex gap-1">
            <Button variant="quiet" disabled={filtered.length === 0 || activeIndex <= 0} onClick={() => selectAt(activeIndex - 1)}>{t("Previous")}</Button>
            <Button variant="quiet" disabled={filtered.length === 0 || activeIndex >= filtered.length - 1} onClick={() => selectAt(activeIndex + 1)}>{t("Next")}</Button>
          </div>
        </div>
        <label className="grid gap-1 text-xs font-medium">
          <span className="sr-only">{t("Search changed files")}</span>
          <input
            type="search" dir="auto"
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            placeholder={t("Search changed files")}
            className="min-h-10 rounded-lg border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
          />
        </label>
        <div className="grid grid-cols-2 gap-2">
          <label className="grid gap-1 text-xs font-medium text-muted-foreground">
            {t("Status")}<select className="min-h-10 rounded-lg border border-border bg-background px-2 text-sm text-foreground" value={status} onChange={(event) => setStatus(event.currentTarget.value as StatusFilter)}>
              <option value="all">{t("All statuses")}</option>
              {Object.entries(statusLabels).map(([value, label]) => <option key={value} value={value}>{t(label)}</option>)}
            </select>
          </label>
          <label className="grid gap-1 text-xs font-medium text-muted-foreground">
            {t("File type")}<select className="min-h-10 rounded-lg border border-border bg-background px-2 text-sm text-foreground" value={kind} onChange={(event) => setKind(event.currentTarget.value as KindFilter)}>
              <option value="all">{t("All types")}</option>
              {kinds.map((value) => <option key={value} value={value}>{value}</option>)}
            </select>
          </label>
        </div>
      </div>
      <div className="grid max-h-72 gap-3 overflow-y-auto p-2 xl:max-h-[44rem]">
        {groups.map(([folder, folderChanges]) => {
          const uniqueFolderKey = folderChanges[0]?.id.replaceAll(/[^a-zA-Z0-9_-]/gu, "-") ?? "empty";
          const headingId = `change-folder-${instanceId}-${folder.replaceAll(/[^a-zA-Z0-9_-]/gu, "-")}-${uniqueFolderKey}`;
          return (
            <section key={folder} aria-labelledby={headingId}>
              <h3 id={headingId} className="sticky top-0 z-10 truncate bg-card/95 px-2 py-1 text-xs font-semibold text-muted-foreground" title={folder}>{folder}</h3>
              <div className="mt-1 grid gap-1">
                {folderChanges.map((change) => {
                  const index = filtered.indexOf(change);
                  const selected = change.id === selectedChangeId;
                  const roving = selected || (selectedIndex < 0 && index === 0);
                  return (
                    <button
                      type="button"
                      key={change.id}
                      data-change-option={change.id}
                      aria-pressed={selected}
                      tabIndex={roving ? 0 : -1}
                      className={selected
                        ? "rounded-lg border border-sky-300/35 bg-sky-300/10 px-3 py-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                        : "rounded-lg border border-transparent px-3 py-3 text-left hover:bg-muted/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"}
                      onClick={() => onSelect(change.id)}
                      onKeyDown={(event) => moveSelection(event, index)}
                    >
                      <span className="block truncate text-sm font-semibold">{change.path}</span>
                      <span className="mt-2 flex items-center justify-between gap-2">
                        <Badge tone="changed">{statusLabels[changeStatus(change)]}</Badge>
                        <span className="text-xs text-muted-foreground">{change.kindLabel}</span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>
          );
        })}
        {filtered.length === 0 ? (
          <p className="rounded-lg border border-border bg-muted/20 p-4 text-sm text-muted-foreground" role="status">{t("No changed files match these filters.")}</p>
        ) : null}
      </div>
    </nav>
  );
}

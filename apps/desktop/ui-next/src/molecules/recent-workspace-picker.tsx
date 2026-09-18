import { Button } from "../atoms/button";
import type { RecentWorkspace, WorkspaceEntryIntent } from "../models/workspace-entry";

function optionLabel(workspace: RecentWorkspace): string {
  if (workspace.state === "current") return `Current · ${workspace.label}`;
  if (workspace.state === "unavailable") return `Unavailable · ${workspace.label}`;
  if (workspace.state === "agent-assigned") return `${workspace.label} · Agent assigned`;
  return workspace.label;
}

export function RecentWorkspacePicker({
  recents,
  selectedPath,
  canSelect,
  hint,
  openLabel,
  canOpen,
  canForget,
  forgetTitle,
  onIntent,
}: Readonly<{
  recents: readonly RecentWorkspace[];
  selectedPath: string;
  canSelect: boolean;
  hint: string;
  openLabel: string;
  canOpen: boolean;
  canForget: boolean;
  forgetTitle: string;
  onIntent: (intent: WorkspaceEntryIntent) => void;
}>) {
  let recentSelect: HTMLSelectElement | null = null;
  const currentSelectedPath = () => recentSelect?.value ?? selectedPath;

  return (
    <div className="space-y-2">
      <label className="block text-sm font-semibold" htmlFor="workspace-entry-recent">Recent workspaces</label>
      <div className="grid gap-2">
        <select
          ref={(element) => { recentSelect = element; }}
          id="workspace-entry-recent"
          value={selectedPath}
          disabled={!canSelect}
          aria-describedby="workspace-entry-recent-hint"
          className="min-h-11 min-w-0 flex-1 rounded-lg border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          onChange={(event) => onIntent({ type: "select-recent", path: event.currentTarget.value })}
        >
          {recents.length === 0 ? <option value="">No recent workspace yet</option> : null}
          {recents.map((workspace) => (
            <option key={workspace.path} value={workspace.path}>{optionLabel(workspace)}</option>
          ))}
        </select>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <Button
            className="w-full"
            variant="secondary"
            disabled={!canOpen}
            aria-describedby="workspace-entry-recent-hint"
            onClick={() => onIntent({ type: "open-recent", path: currentSelectedPath() })}
          >
            {openLabel}
          </Button>
          <Button
            className="w-full"
            variant="quiet"
            disabled={!canForget}
            title={forgetTitle || undefined}
            aria-describedby="workspace-entry-recent-hint"
            onClick={() => onIntent({ type: "forget-recent", path: currentSelectedPath() })}
          >
            Forget from list
          </Button>
        </div>
      </div>
      <p id="workspace-entry-recent-hint" className="text-xs leading-5 text-muted-foreground" aria-live="polite">{hint}</p>
    </div>
  );
}

import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { Card, CardContent } from "../atoms/card";
import { RecentWorkspacePicker } from "../molecules/recent-workspace-picker";
import type { WorkspaceEntryIntent, WorkspaceEntryModel } from "../models/workspace-entry";

export function WorkspaceEntry({ model, onIntent }: Readonly<{
  model: WorkspaceEntryModel;
  onIntent: (intent: WorkspaceEntryIntent) => void;
}>) {
  const submitManagedPath = (path: string) => {
    if (path.length > 0) onIntent({ type: "open-managed-path", path });
  };

  return (
    <section aria-labelledby="workspace-entry-title" className="py-6 sm:py-8 lg:py-10">
      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1.2fr)_minmax(22rem,0.8fr)] lg:gap-10">
        <div className="max-w-3xl pt-2">
          <Badge tone={model.mode === "ready" ? "positive" : "neutral"}>{model.eyebrow}</Badge>
          <h1 id="workspace-entry-title" className="mt-4 text-3xl font-bold tracking-tight text-foreground sm:text-4xl lg:text-5xl">{model.title}</h1>
          <p className="mt-4 max-w-2xl text-base leading-7 text-muted-foreground sm:text-lg">{model.description}</p>
          <div className="mt-6 flex flex-col gap-3 sm:flex-row sm:items-center">
            <Button variant="primary" disabled={!model.canChoose} onClick={() => onIntent({ type: "choose-folder" })}>
              {model.chooseLabel}
            </Button>
            <Button
              variant="secondary"
              disabled={!model.canRetry}
              onClick={() => onIntent({ type: "retry" })}
            >
              {model.retryLabel}
            </Button>
            <span className="text-xs leading-5 text-muted-foreground">Your original folder stays untouched.</span>
          </div>
        </div>

        <Card className="overflow-hidden">
          <CardContent className="p-0">
            <details
              open={model.disclosureOpen}
              onToggle={(event) => onIntent({ type: "set-disclosure", open: event.currentTarget.open })}
              className="group"
            >
              <summary className="flex min-h-11 cursor-pointer list-none items-center justify-between gap-3 px-5 py-4 text-sm font-semibold outline-none marker:content-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
                {model.disclosureLabel}
                <span aria-hidden="true" className="text-muted-foreground transition-transform group-open:rotate-180">⌄</span>
              </summary>
              <div className="space-y-5 border-t border-border p-5">
                <Button className="w-full" variant="secondary" disabled={!model.canChooseManaged} onClick={() => onIntent({ type: "choose-managed-folder" })}>
                  Choose managed workspace
                </Button>

                <form
                  className="space-y-2"
                  onSubmit={(event) => {
                    event.preventDefault();
                    submitManagedPath(event.currentTarget.querySelector<HTMLInputElement>("#workspace-entry-path")?.value ?? model.openPath);
                  }}
                >
                  <label className="block text-sm font-semibold" htmlFor="workspace-entry-path">Or enter its path</label>
                  <div className="flex flex-col gap-2 sm:flex-row">
                    <input
                      id="workspace-entry-path"
                      value={model.openPath}
                      disabled={!model.canEditPath}
                      autoComplete="off"
                      placeholder="/absolute/path/to/managed-workspace"
                      className="min-h-11 min-w-0 flex-1 rounded-lg border border-border bg-background px-3 text-sm text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                      onChange={(event) => {
                        const nextPath = event.currentTarget.value;
                        onIntent({ type: "update-managed-path", path: nextPath });
                      }}
                      onKeyDown={(event) => {
                        if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
                        event.preventDefault();
                        submitManagedPath(event.currentTarget.value);
                      }}
                    />
                    <Button variant="secondary" type="submit" disabled={!model.canOpenPath || model.openPath.length === 0}>Open path</Button>
                  </div>
                </form>

                <RecentWorkspacePicker
                  recents={model.recents}
                  selectedPath={model.selectedRecentPath}
                  canSelect={model.canSelectRecent}
                  hint={model.recentHint}
                  openLabel={model.recentOpenLabel}
                  canOpen={model.canOpenRecent}
                  canForget={model.canForgetRecent}
                  forgetTitle={model.forgetRecentTitle}
                  onIntent={onIntent}
                />
              </div>
            </details>
          </CardContent>
        </Card>
      </div>
    </section>
  );
}

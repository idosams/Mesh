import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";

export function WorkspaceCommandBar() {
  return (
    <div className="flex flex-col gap-4 rounded-xl border border-border bg-card/75 p-4 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <div className="flex items-center gap-2">
          <span className="h-2 w-2 rounded-full bg-emerald-300 shadow-[0_0_12px_rgba(110,231,183,.75)]" />
          <strong className="text-sm">Compensation planning</strong>
          <Badge tone="positive">Saved locally</Badge>
        </div>
        <p className="mt-1 text-xs text-muted-foreground">3 files changed · exact private version 4b7…e91</p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="quiet">Open folder</Button>
        <Button variant="secondary">Save version</Button>
        <Button variant="primary">Start review</Button>
      </div>
    </div>
  );
}

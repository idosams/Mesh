import { WorkspaceEntryShell } from "../layouts/workspace-entry-shell";
import type { WorkspaceEntryIntent, WorkspaceEntryModel } from "../models/workspace-entry";
import { WorkspaceEntry } from "../organisms/workspace-entry";

export function WorkspaceEntryPage({ model, onIntent }: Readonly<{
  model: WorkspaceEntryModel;
  onIntent: (intent: WorkspaceEntryIntent) => void;
}>) {
  return <WorkspaceEntryShell><WorkspaceEntry model={model} onIntent={onIntent} /></WorkspaceEntryShell>;
}

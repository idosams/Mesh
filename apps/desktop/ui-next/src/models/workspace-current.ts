export type WorkspaceCurrentActionId =
  | "refresh"
  | "open-folder"
  | "open-version"
  | "start-codex"
  | "start-agent-copy"
  | "update-destination"
  | "finish-agent"
  | "return-workspace"
  | "open-terminal"
  | "copy-diagnostics"
  | "copy-working-path"
  | "copy-agent-path"
  | "rollback";

export type WorkspaceCurrentAction = Readonly<{
  id: WorkspaceCurrentActionId;
  label: string;
  enabled: boolean;
}>;

export type WorkspaceCurrentModel = Readonly<{
  state: "Working" | "Saved privately" | "Available to team" | "Ready for review" | "Needs attention" | "Approved";
  recordSummary: string;
  workingFolder: string;
  agentFolderLabel: string;
  agentFolder: string;
  agentAssigned: boolean;
  nativeFolderHint: string;
  privateVersion: string;
  privateVersionTitle: string;
  sharedVersion: string;
  sharedVersionTitle: string;
  destination: string;
  entryCount: number;
  entries: readonly string[];
  conditions: readonly string[];
  actions: readonly WorkspaceCurrentAction[];
}>;

export type WorkspaceCurrentIntent = Readonly<{
  type: "activate";
  action: WorkspaceCurrentActionId;
}>;

type JsonRecord = Record<string, unknown>;

const STATES: readonly WorkspaceCurrentModel["state"][] = Object.freeze([
  "Working",
  "Saved privately",
  "Available to team",
  "Ready for review",
  "Needs attention",
  "Approved",
]);

const ACTION_IDS: readonly WorkspaceCurrentActionId[] = Object.freeze([
  "refresh",
  "open-folder",
  "open-version",
  "start-codex",
  "start-agent-copy",
  "update-destination",
  "finish-agent",
  "return-workspace",
  "open-terminal",
  "copy-diagnostics",
  "copy-working-path",
  "copy-agent-path",
  "rollback",
]);

const REQUIRED_ACTION_IDS: readonly WorkspaceCurrentActionId[] = Object.freeze([
  "refresh",
  "open-folder",
  "open-version",
  "start-codex",
  "start-agent-copy",
  "update-destination",
  "open-terminal",
  "copy-diagnostics",
  "copy-working-path",
  "copy-agent-path",
  "rollback",
]);

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value as JsonRecord;
}

function exactKeys(value: JsonRecord, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length
    || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

function safeText(value: unknown, label: string, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function safeList(value: unknown, label: string, maximumItems: number, maximumText: number): readonly string[] {
  if (!Array.isArray(value) || value.length > maximumItems) {
    throw new Error(`${label} was invalid or unbounded.`);
  }
  return Object.freeze(value.map((item, index) => safeText(item, `${label} item ${index + 1}`, maximumText)));
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

function count(value: unknown, label: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0 || (value as number) > 1_000_000) {
    throw new Error(`${label} was invalid or unbounded.`);
  }
  return value as number;
}

function actions(value: unknown): readonly WorkspaceCurrentAction[] {
  if (!Array.isArray(value) || value.length > ACTION_IDS.length) {
    throw new Error("workspace current actions were invalid or unbounded.");
  }
  const seen = new Set<WorkspaceCurrentActionId>();
  const parsed = value.map((candidate, index) => {
    const action = record(candidate, `workspace current action ${index + 1}`);
    exactKeys(action, ["enabled", "id", "label"], `workspace current action ${index + 1}`);
    if (!ACTION_IDS.includes(action.id as WorkspaceCurrentActionId)) {
      throw new Error("A workspace current action was not recognized.");
    }
    const id = action.id as WorkspaceCurrentActionId;
    if (seen.has(id)) throw new Error("A workspace current action was duplicated.");
    seen.add(id);
    return Object.freeze({
      id,
      label: safeText(action.label, `workspace current ${id} label`, 96),
      enabled: boolean(action.enabled, `workspace current ${id} authority`),
    });
  });
  if (REQUIRED_ACTION_IDS.some((id) => !seen.has(id))) {
    throw new Error("The workspace current projection omitted an established control.");
  }
  return Object.freeze(parsed);
}

export function workspaceCurrentEnvelope(
  value: unknown,
  previousGeneration: number,
): Readonly<{ generation: number; model: WorkspaceCurrentModel }> {
  const envelope = record(value, "workspace current envelope");
  exactKeys(envelope, ["current", "generation"], "workspace current envelope");
  if (!Number.isSafeInteger(envelope.generation)
    || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace current generation was stale or invalid.");
  }
  const current = record(envelope.current, "workspace current projection");
  exactKeys(current, [
    "actions",
    "agentAssigned",
    "agentFolder",
    "agentFolderLabel",
    "conditions",
    "destination",
    "entries",
    "entryCount",
    "nativeFolderHint",
    "privateVersion",
    "privateVersionTitle",
    "recordSummary",
    "sharedVersion",
    "sharedVersionTitle",
    "state",
    "workingFolder",
  ], "workspace current projection");
  if (!STATES.includes(current.state as WorkspaceCurrentModel["state"])) {
    throw new Error("The workspace current state was not recognized.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      state: current.state as WorkspaceCurrentModel["state"],
      recordSummary: safeText(current.recordSummary, "record summary", 160),
      workingFolder: safeText(current.workingFolder, "working folder", 4_096),
      agentFolderLabel: safeText(current.agentFolderLabel, "agent folder label", 96),
      agentFolder: safeText(current.agentFolder, "agent folder", 4_096),
      agentAssigned: boolean(current.agentAssigned, "agent custody"),
      nativeFolderHint: safeText(current.nativeFolderHint, "native folder guidance", 4_096),
      privateVersion: safeText(current.privateVersion, "private version", 256),
      privateVersionTitle: safeText(current.privateVersionTitle, "private version detail", 512),
      sharedVersion: safeText(current.sharedVersion, "shared version", 256),
      sharedVersionTitle: safeText(current.sharedVersionTitle, "shared version detail", 512),
      destination: safeText(current.destination, "destination", 4_096),
      entryCount: count(current.entryCount, "entry count"),
      entries: safeList(current.entries, "materialized paths", 10_000, 4_096),
      conditions: safeList(current.conditions, "workspace conditions", 2_048, 2_048),
      actions: actions(current.actions),
    }),
  });
}

export function workspaceCurrentIntent(value: unknown): WorkspaceCurrentIntent {
  const intent = record(value, "workspace current intent");
  exactKeys(intent, ["action", "type"], "workspace current intent");
  if (intent.type !== "activate" || !ACTION_IDS.includes(intent.action as WorkspaceCurrentActionId)) {
    throw new Error("The workspace current intent was not recognized.");
  }
  return Object.freeze({ type: "activate", action: intent.action as WorkspaceCurrentActionId });
}

import statusCatalog from "../../../src/strings/status.json";

export type WorkspaceOverviewAction = "recommended" | "open-another-version" | "open-folder" | "find-changes" | "open-review" | "return-workspace" | "copy-diagnostics";

export type WorkspaceOverviewModel = Readonly<{
  workspaceName: string;
  state: "Working" | "Saved privately" | "Available to team" | "Ready for review" | "Needs attention" | "Approved";
  workingFolder: string;
  recordSummary: string;
  privateVersion: string;
  sharedVersion: string;
  nativeChangeCount: number;
  savedVersionCount: number;
  nextActionTitle: string;
  nextActionDescription: string;
  nextActionLabel: string;
  nextActionDisabled: boolean;
  canOpenAnotherVersion: boolean;
  canOpenFolder: boolean;
  canFindChanges: boolean;
  canOpenReview: boolean;
  canReturnWorkspace: boolean;
  canCopyDiagnostics: boolean;
}>;

export type WorkspaceOverviewIntent = Readonly<{ type: WorkspaceOverviewAction }>;

type JsonRecord = Record<string, unknown>;

const PRODUCT_STATES: readonly string[] = Object.freeze(
  statusCatalog.states.map((state) => state.status),
);

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

function boundedCount(value: unknown, label: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0 || (value as number) > 1_000_000) {
    throw new Error(`${label} was invalid or unbounded.`);
  }
  return value as number;
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

export function workspaceOverviewEnvelope(
  value: unknown,
  previousGeneration: number,
): Readonly<{ generation: number; model: WorkspaceOverviewModel }> {
  const envelope = record(value, "workspace overview envelope");
  exactKeys(envelope, ["generation", "overview"], "workspace overview envelope");
  if (!Number.isSafeInteger(envelope.generation)
    || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace overview generation was stale or invalid.");
  }
  const overview = record(envelope.overview, "workspace overview");
  exactKeys(overview, [
    "canFindChanges",
    "canCopyDiagnostics",
    "canOpenAnotherVersion",
    "canOpenFolder",
    "canOpenReview",
    "canReturnWorkspace",
    "nativeChangeCount",
    "nextActionDescription",
    "nextActionDisabled",
    "nextActionLabel",
    "nextActionTitle",
    "privateVersion",
    "recordSummary",
    "savedVersionCount",
    "sharedVersion",
    "state",
    "workingFolder",
    "workspaceName",
  ], "workspace overview");
  if (!PRODUCT_STATES.includes(overview.state as string)) {
    throw new Error("The workspace overview state was not in the closed product vocabulary.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      workspaceName: safeText(overview.workspaceName, "workspace name", 256),
      state: overview.state as WorkspaceOverviewModel["state"],
      workingFolder: safeText(overview.workingFolder, "working folder", 4_096),
      recordSummary: safeText(overview.recordSummary, "record summary", 160),
      privateVersion: safeText(overview.privateVersion, "private version", 256),
      sharedVersion: safeText(overview.sharedVersion, "shared version", 256),
      nativeChangeCount: boundedCount(overview.nativeChangeCount, "native change count"),
      savedVersionCount: boundedCount(overview.savedVersionCount, "saved version count"),
      nextActionTitle: safeText(overview.nextActionTitle, "next action title", 160),
      nextActionDescription: safeText(overview.nextActionDescription, "next action description", 1_024),
      nextActionLabel: safeText(overview.nextActionLabel, "next action label", 80),
      nextActionDisabled: boolean(overview.nextActionDisabled, "next action disabled"),
      canOpenAnotherVersion: boolean(overview.canOpenAnotherVersion, "open-another-version authority"),
      canOpenFolder: boolean(overview.canOpenFolder, "open-folder authority"),
      canFindChanges: boolean(overview.canFindChanges, "find-changes authority"),
      canOpenReview: boolean(overview.canOpenReview, "open-review authority"),
      canReturnWorkspace: boolean(overview.canReturnWorkspace, "return-workspace authority"),
      canCopyDiagnostics: boolean(overview.canCopyDiagnostics, "copy-diagnostics authority"),
    }),
  });
}

export function workspaceOverviewIntent(value: unknown): WorkspaceOverviewIntent {
  const intent = record(value, "workspace overview intent");
  exactKeys(intent, ["type"], "workspace overview intent");
  const actions: readonly WorkspaceOverviewAction[] = [
    "recommended",
    "open-another-version",
    "open-folder",
    "find-changes",
    "open-review",
    "return-workspace",
    "copy-diagnostics",
  ];
  if (!actions.includes(intent.type as WorkspaceOverviewAction)) {
    throw new Error("The workspace overview intent was not recognized.");
  }
  return Object.freeze({ type: intent.type as WorkspaceOverviewAction });
}

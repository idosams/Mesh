export type WorkspaceVersionRelation = "current" | "earlier" | "concurrent";

export type WorkspaceVersionItem = Readonly<{
  operation: string;
  ordinal: number;
  relation: WorkspaceVersionRelation;
  label: string;
}>;

export type WorkspaceVersionsModel = Readonly<{
  historyMode: "linear" | "concurrent";
  versions: readonly WorkspaceVersionItem[];
  selectedOperation: string | null;
  previewState: "choose" | "loading" | "ready" | "error";
  previewTitle: string;
  previewSummary: string;
  changeBasis: "initial" | "previous-point" | "combined-history" | null;
  basisOrdinal: number | null;
  changes: readonly string[];
  entries: readonly string[];
  canSelect: boolean;
  canOpen: boolean;
  canStartCodex: boolean;
  customLocation: string;
  canUseCustomLocation: boolean;
  openLabel: string;
  codexLabel: string;
}>;

export type WorkspaceVersionsIntent =
  | Readonly<{ type: "select-version"; operation: string }>
  | Readonly<{ type: "open-version"; operation: string }>
  | Readonly<{ type: "start-codex"; operation: string }>
  | Readonly<{ type: "set-custom-location"; path: string }>;

type JsonRecord = Record<string, unknown>;
const OPERATION = /^[0-9a-f]{64}$/u;
const UNSAFE_TEXT = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;
const UNSAFE_PATH = /[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;

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
  if (typeof value !== "string" || value.length === 0 || value.length > maximum || UNSAFE_TEXT.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function safePath(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length > 4_096 || UNSAFE_PATH.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function operation(value: unknown, label: string): string {
  if (typeof value !== "string" || !OPERATION.test(value)) {
    throw new Error(`${label} was not an exact saved-version identity.`);
  }
  return value;
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

function lines(value: unknown, label: string): readonly string[] {
  if (!Array.isArray(value) || value.length > 25) {
    throw new Error(`${label} was invalid or unbounded.`);
  }
  return Object.freeze(value.map((line, index) => safeText(line, `${label} line ${index + 1}`, 4_096)));
}

export function workspaceVersionsEnvelope(
  value: unknown,
  previousGeneration: number,
): Readonly<{ generation: number; model: WorkspaceVersionsModel }> {
  const envelope = record(value, "workspace versions envelope");
  exactKeys(envelope, ["generation", "versions"], "workspace versions envelope");
  if (!Number.isSafeInteger(envelope.generation)
    || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace versions generation was stale or invalid.");
  }
  const source = record(envelope.versions, "workspace versions");
  exactKeys(source, [
    "canOpen",
    "canSelect",
    "canStartCodex",
    "canUseCustomLocation",
    "basisOrdinal",
    "changeBasis",
    "changes",
    "codexLabel",
    "customLocation",
    "entries",
    "historyMode",
    "openLabel",
    "previewState",
    "previewSummary",
    "previewTitle",
    "selectedOperation",
    "versions",
  ], "workspace versions");
  if (source.historyMode !== "linear" && source.historyMode !== "concurrent") {
    throw new Error("The workspace history mode was not recognized.");
  }
  if (!Array.isArray(source.versions) || source.versions.length > 256) {
    throw new Error("The workspace version list was invalid or unbounded.");
  }
  const seen = new Set<string>();
  const versions = Object.freeze(source.versions.map((candidate, index) => {
    const item = record(candidate, `workspace version ${index + 1}`);
    exactKeys(item, ["label", "operation", "ordinal", "relation"], `workspace version ${index + 1}`);
    const exactOperation = operation(item.operation, `workspace version ${index + 1} operation`);
    if (seen.has(exactOperation)) throw new Error("The workspace version list repeated an identity.");
    seen.add(exactOperation);
    if (!Number.isSafeInteger(item.ordinal) || (item.ordinal as number) < 1) {
      throw new Error("The workspace version ordinal was invalid.");
    }
    if (item.relation !== "current" && item.relation !== "earlier" && item.relation !== "concurrent") {
      throw new Error("The workspace version relation was not recognized.");
    }
    return Object.freeze({
      operation: exactOperation,
      ordinal: item.ordinal as number,
      relation: item.relation,
      label: safeText(item.label, `workspace version ${index + 1} label`, 160),
    });
  }));
  const selectedOperation = source.selectedOperation === null
    ? null
    : operation(source.selectedOperation, "selected workspace version");
  if (selectedOperation !== null && !seen.has(selectedOperation)) {
    throw new Error("The selected workspace version was not in the bounded list.");
  }
  const previewStates = ["choose", "loading", "ready", "error"] as const;
  if (!previewStates.includes(source.previewState as typeof previewStates[number])) {
    throw new Error("The workspace version preview state was not recognized.");
  }
  const changes = lines(source.changes, "workspace version changes");
  const entries = lines(source.entries, "workspace version entries");
  const changeBases = ["initial", "previous-point", "combined-history"] as const;
  const changeBasis = source.changeBasis === null
    ? null
    : changeBases.includes(source.changeBasis as typeof changeBases[number])
      ? source.changeBasis as WorkspaceVersionsModel["changeBasis"]
      : (() => { throw new Error("The workspace version change basis was not recognized."); })();
  const basisOrdinal = source.basisOrdinal === null
    ? null
    : Number.isSafeInteger(source.basisOrdinal) && (source.basisOrdinal as number) > 0
      ? source.basisOrdinal as number
      : (() => { throw new Error("The workspace version basis ordinal was invalid."); })();
  const canSelect = boolean(source.canSelect, "select-version authority");
  const canOpen = boolean(source.canOpen, "open-version authority");
  const canStartCodex = boolean(source.canStartCodex, "start-codex authority");
  if ((canOpen || canStartCodex) && (source.previewState !== "ready" || selectedOperation === null)) {
    throw new Error("Workspace version authority was offered without an exact verified preview.");
  }
  if (source.previewState !== "ready" && (changes.length !== 0 || entries.length !== 0)) {
    throw new Error("Unverified workspace version details were presented as exact content.");
  }
  if ((source.previewState === "ready") !== (changeBasis !== null)
    || (changeBasis === "previous-point") !== (basisOrdinal !== null)) {
    throw new Error("Workspace version change-basis truth did not match the exact preview.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      historyMode: source.historyMode as WorkspaceVersionsModel["historyMode"],
      versions,
      selectedOperation,
      previewState: source.previewState as WorkspaceVersionsModel["previewState"],
      previewTitle: safeText(source.previewTitle, "workspace version preview title", 160),
      previewSummary: safeText(source.previewSummary, "workspace version preview summary", 1_024),
      changeBasis,
      basisOrdinal,
      changes,
      entries,
      canSelect,
      canOpen,
      canStartCodex,
      customLocation: safePath(source.customLocation, "custom version location"),
      canUseCustomLocation: boolean(source.canUseCustomLocation, "custom-location availability"),
      openLabel: safeText(source.openLabel, "open-version label", 80),
      codexLabel: safeText(source.codexLabel, "start-codex label", 80),
    }),
  });
}

export function workspaceVersionsIntent(value: unknown): WorkspaceVersionsIntent {
  const intent = record(value, "workspace versions intent");
  if (intent.type === "set-custom-location") {
    exactKeys(intent, ["path", "type"], "workspace versions intent");
    return Object.freeze({
      type: "set-custom-location",
      path: safePath(intent.path, "workspace version custom location"),
    });
  }
  exactKeys(intent, ["operation", "type"], "workspace versions intent");
  if (intent.type !== "select-version" && intent.type !== "open-version" && intent.type !== "start-codex") {
    throw new Error("The workspace versions intent was not recognized.");
  }
  return Object.freeze({
    type: intent.type,
    operation: operation(intent.operation, "workspace versions intent operation"),
  });
}

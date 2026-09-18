import { BOUNDED_CHOICE_LIMIT, boundedChoiceProjection, type BoundedChoiceProjection } from "./choice-projection";

export type RestoreFileFormat = "Text" | "PDF" | "Word" | "PowerPoint" | "Excel" | "File";

export type RestoreFileChoice = Readonly<{
  id: string;
  path: string;
  label: string;
  format: RestoreFileFormat;
}>;

export type RestoreVersionChoice = Readonly<{
  id: string;
  label: string;
}>;

export type RestoreFileProjection = BoundedChoiceProjection<RestoreFileChoice>;
export type RestoreVersionProjection = BoundedChoiceProjection<RestoreVersionChoice>;

export function workspaceRestoreFileProjection(
  choices: readonly RestoreFileChoice[],
  filterText: string,
  selectedId: string,
  maximum = BOUNDED_CHOICE_LIMIT,
): RestoreFileProjection {
  return boundedChoiceProjection(
    choices,
    filterText,
    selectedId,
    (choice) => choice.id,
    (choice) => [choice.path, choice.label, choice.format],
    maximum,
  );
}

export function workspaceRestoreVersionProjection(
  choices: readonly RestoreVersionChoice[],
  filterText: string,
  selectedId: string,
  maximum = BOUNDED_CHOICE_LIMIT,
): RestoreVersionProjection {
  return boundedChoiceProjection(
    choices,
    filterText,
    selectedId,
    (choice) => choice.id,
    (choice) => [choice.id, choice.label],
    maximum,
  );
}

export type RestorePreview = Readonly<{
  filePath: string;
  format: RestoreFileFormat;
  currentVersion: string;
  targetVersion: string;
  change: string;
  historyNote: string;
  undoNote: string;
}>;

export type WorkspaceRestoreModel = Readonly<{
  files: readonly RestoreFileChoice[];
  selectedFileId: string;
  versions: readonly RestoreVersionChoice[];
  selectedVersionId: string;
  canSelectFile: boolean;
  canSelectVersion: boolean;
  canPreview: boolean;
  canApply: boolean;
  canUndo: boolean;
  hint: string;
  preview: RestorePreview | null;
  undoLabel: string;
}>;

export type WorkspaceRestoreIntent =
  | Readonly<{ type: "select-file"; id: string }>
  | Readonly<{ type: "select-version"; id: string }>
  | Readonly<{ type: "preview" }>
  | Readonly<{ type: "apply" }>
  | Readonly<{ type: "undo" }>;

type JsonRecord = Record<string, unknown>;

const FORMATS: readonly RestoreFileFormat[] = Object.freeze(["Text", "PDF", "Word", "PowerPoint", "Excel", "File"]);
const unsafeText = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value as JsonRecord;
}

function exactKeys(value: JsonRecord, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

function safeText(value: unknown, label: string, maximum: number, empty = false): string {
  if (typeof value !== "string" || (!empty && value.length === 0) || value.length > maximum || unsafeText.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

function format(value: unknown, label: string): RestoreFileFormat {
  if (!FORMATS.includes(value as RestoreFileFormat)) throw new Error(`${label} was invalid.`);
  return value as RestoreFileFormat;
}

function fileChoice(value: unknown): RestoreFileChoice {
  const choice = record(value, "restore file choice");
  exactKeys(choice, ["format", "id", "label", "path"], "restore file choice");
  return Object.freeze({
    id: safeText(choice.id, "restore file identity", 256),
    path: safeText(choice.path, "restore file path", 4_096),
    label: safeText(choice.label, "restore file label", 4_608),
    format: format(choice.format, "restore file format"),
  });
}

function versionChoice(value: unknown): RestoreVersionChoice {
  const choice = record(value, "restore version choice");
  exactKeys(choice, ["id", "label"], "restore version choice");
  return Object.freeze({
    id: safeText(choice.id, "restore version identity", 256),
    label: safeText(choice.label, "restore version label", 512),
  });
}

function restorePreview(value: unknown): RestorePreview | null {
  if (value === null) return null;
  const preview = record(value, "restore preview");
  exactKeys(preview, ["change", "currentVersion", "filePath", "format", "historyNote", "targetVersion", "undoNote"], "restore preview");
  return Object.freeze({
    filePath: safeText(preview.filePath, "restore preview file path", 4_096),
    format: format(preview.format, "restore preview format"),
    currentVersion: safeText(preview.currentVersion, "restore current version", 256),
    targetVersion: safeText(preview.targetVersion, "restore target version", 256),
    change: safeText(preview.change, "restore change", 1_000),
    historyNote: safeText(preview.historyNote, "restore history note", 1_000),
    undoNote: safeText(preview.undoNote, "restore undo note", 1_000),
  });
}

export function workspaceRestoreEnvelope(value: unknown, previousGeneration: number): Readonly<{
  generation: number;
  model: WorkspaceRestoreModel;
}> {
  const envelope = record(value, "workspace restore envelope");
  exactKeys(envelope, ["generation", "restore"], "workspace restore envelope");
  if (!Number.isSafeInteger(envelope.generation) || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace restore generation was stale or invalid.");
  }
  const restore = record(envelope.restore, "workspace restore");
  exactKeys(restore, [
    "canApply", "canPreview", "canSelectFile", "canSelectVersion", "canUndo", "files", "hint",
    "preview", "selectedFileId", "selectedVersionId", "undoLabel", "versions",
  ], "workspace restore");
  if (!Array.isArray(restore.files) || restore.files.length > 4_096
    || !Array.isArray(restore.versions) || restore.versions.length > 4_096) {
    throw new Error("The workspace restore choices were invalid or unbounded.");
  }
  const files = Object.freeze(restore.files.map(fileChoice));
  const versions = Object.freeze(restore.versions.map(versionChoice));
  if (new Set(files.map((choice) => choice.id)).size !== files.length
    || new Set(versions.map((choice) => choice.id)).size !== versions.length) {
    throw new Error("The workspace restore choices were not unique.");
  }
  const selectedFileId = safeText(restore.selectedFileId, "selected restore file", 256, true);
  const selectedVersionId = safeText(restore.selectedVersionId, "selected restore version", 256, true);
  if ((selectedFileId && !files.some((choice) => choice.id === selectedFileId))
    || (selectedVersionId && !versions.some((choice) => choice.id === selectedVersionId))) {
    throw new Error("The workspace restore selection was not in the projected choices.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      files,
      selectedFileId,
      versions,
      selectedVersionId,
      canSelectFile: boolean(restore.canSelectFile, "restore file selection availability"),
      canSelectVersion: boolean(restore.canSelectVersion, "restore version selection availability"),
      canPreview: boolean(restore.canPreview, "restore preview availability"),
      canApply: boolean(restore.canApply, "restore apply availability"),
      canUndo: boolean(restore.canUndo, "restore undo availability"),
      hint: safeText(restore.hint, "restore hint", 1_000),
      preview: restorePreview(restore.preview),
      undoLabel: safeText(restore.undoLabel, "restore undo label", 160),
    }),
  });
}

export function workspaceRestoreIntent(value: unknown): WorkspaceRestoreIntent {
  const intent = record(value, "workspace restore intent");
  if (["preview", "apply", "undo"].includes(intent.type as string)) {
    exactKeys(intent, ["type"], "workspace restore intent");
    return Object.freeze({ type: intent.type }) as WorkspaceRestoreIntent;
  }
  if (["select-file", "select-version"].includes(intent.type as string)) {
    exactKeys(intent, ["id", "type"], "workspace restore intent");
    return Object.freeze({
      type: intent.type,
      id: safeText(intent.id, "workspace restore intent identity", 256),
    }) as WorkspaceRestoreIntent;
  }
  throw new Error("The workspace restore intent was not recognized.");
}

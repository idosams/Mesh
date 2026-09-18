export type WorkspaceWorkActionId =
  | "create-text"
  | "create-folder"
  | "move-entry"
  | "delete-entry"
  | "open-entry"
  | "reveal-entry"
  | "open-workspace-folder"
  | "scan-files"
  | "load-file"
  | "preserve-edit"
  | "save-private"
  | "save-all-private"
  | "record-structural-change";

export type WorkspaceWorkField =
  | "newPath"
  | "selectedEntry"
  | "movePath"
  | "selectedFile"
  | "editorText"
  | "missingSource"
  | "moveTarget";

export type WorkspaceWorkAction = Readonly<{ id: WorkspaceWorkActionId; label: string; enabled: boolean }>;
export type WorkspaceWorkChoice = Readonly<{ value: string; label: string }>;
export type WorkspaceEntryChoice = Readonly<{
  value: string;
  label: string;
  kind: "file" | "folder";
}>;

export type WorkspaceNativeChange = Readonly<{
  path: string;
  description: string;
  detail: string;
  code: "A" | "D" | "M" | "?";
  status: "Added" | "Deleted or moved" | "Modified" | "Unsupported";
}>;

export type WorkspaceFilesChangesModel = Readonly<{
  files: Readonly<{
    workspaceLabel: string;
    workspaceRoot: string;
    workspaceState: "current" | "agent-assigned";
    entries: readonly WorkspaceEntryChoice[];
    newPath: string;
    selectedEntry: string;
    movePath: string;
    canEditNewPath: boolean;
    canSelectEntry: boolean;
    canEditMovePath: boolean;
    status: string;
  }>;
  changes: Readonly<{
    files: readonly WorkspaceWorkChoice[];
    selectedFile: string;
    canSelectFile: boolean;
    editorKind: "none" | "text" | "binary";
    editorText: string;
    baselineText: string;
    baselineAvailable: boolean;
    canEditText: boolean;
    editState: string;
    editVersion: string;
    scanState: "idle" | "scanning" | "clean" | "changes" | "error";
    queueSummary: string;
    queue: readonly WorkspaceNativeChange[];
    autoSaveChecked: boolean;
    autoSaveEnabled: boolean;
    autoSaveHint: string;
    structural: null | Readonly<{
      missingSources: readonly WorkspaceWorkChoice[];
      missingSource: string;
      moveTargets: readonly WorkspaceWorkChoice[];
      moveTarget: string;
      canChoose: boolean;
      hint: string;
    }>;
  }>;
  actions: readonly WorkspaceWorkAction[];
}>;

export type WorkspaceFilesChangesIntent =
  | Readonly<{ type: "activate"; action: WorkspaceWorkActionId }>
  | Readonly<{
      type: "activate";
      action: "create-text" | "create-folder";
      field: "newPath";
      value: string;
    }>
  | Readonly<{ type: "activate"; action: "move-entry"; field: "movePath"; value: string }>
  | Readonly<{ type: "activate"; action: "load-file"; field: "selectedFile"; value: string }>
  | Readonly<{ type: "activate"; action: "preserve-edit"; field: "editorText"; value: string }>
  | Readonly<{
      type: "activate";
      action: "record-structural-change";
      missingSource: string;
      moveTarget: string;
    }>
  | Readonly<{ type: "set-field"; field: WorkspaceWorkField; value: string }>
  | Readonly<{ type: "set-auto-save"; checked: boolean }>;

type JsonRecord = Record<string, unknown>;

const ACTION_IDS: readonly WorkspaceWorkActionId[] = Object.freeze([
  "create-text", "create-folder", "move-entry", "delete-entry", "open-entry", "reveal-entry",
  "open-workspace-folder", "scan-files",
  "load-file", "preserve-edit", "save-private", "save-all-private", "record-structural-change",
]);
const FIELD_IDS: readonly WorkspaceWorkField[] = Object.freeze([
  "newPath", "selectedEntry", "movePath", "selectedFile", "editorText", "missingSource", "moveTarget",
]);
const ACTION_ECHO_FIELDS: Readonly<Partial<Record<WorkspaceWorkActionId, WorkspaceWorkField>>> = Object.freeze({
  "create-text": "newPath",
  "create-folder": "newPath",
  "move-entry": "movePath",
  "load-file": "selectedFile",
  "preserve-edit": "editorText",
});
const UNSAFE = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} was not one object.`);
  return value as JsonRecord;
}

function exactKeys(value: JsonRecord, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

function text(value: unknown, label: string, maximum: number, empty = false): string {
  if (typeof value !== "string" || (!empty && value.length === 0) || value.length > maximum || UNSAFE.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function bool(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

function choices(value: unknown, label: string): readonly WorkspaceWorkChoice[] {
  if (!Array.isArray(value) || value.length > 10_000) throw new Error(`${label} was invalid or unbounded.`);
  const seen = new Set<string>();
  return Object.freeze(value.map((candidate, index) => {
    const item = record(candidate, `${label} ${index + 1}`);
    exactKeys(item, ["label", "value"], `${label} ${index + 1}`);
    const itemValue = text(item.value, `${label} ${index + 1} value`, 4_096);
    if (seen.has(itemValue)) throw new Error(`${label} repeated a value.`);
    seen.add(itemValue);
    return Object.freeze({ value: itemValue, label: text(item.label, `${label} ${index + 1} label`, 4_096) });
  }));
}

function entries(value: unknown): readonly WorkspaceEntryChoice[] {
  if (!Array.isArray(value) || value.length > 10_000) throw new Error("managed entries were invalid or unbounded.");
  const seen = new Set<string>();
  return Object.freeze(value.map((candidate, index) => {
    const item = record(candidate, `managed entry ${index + 1}`);
    exactKeys(item, ["kind", "label", "value"], `managed entry ${index + 1}`);
    const itemValue = text(item.value, `managed entry ${index + 1} value`, 4_096);
    if (seen.has(itemValue)) throw new Error("managed entries repeated a value.");
    seen.add(itemValue);
    if (item.kind !== "file" && item.kind !== "folder") throw new Error("A managed entry kind was unknown.");
    return Object.freeze({
      value: itemValue,
      label: text(item.label, `managed entry ${index + 1} label`, 4_096),
      kind: item.kind,
    });
  }));
}

const CHANGE_STATUS = Object.freeze({
  A: "Added",
  D: "Deleted or moved",
  M: "Modified",
  "?": "Unsupported",
} as const);

function nativeChanges(value: unknown): readonly WorkspaceNativeChange[] {
  if (!Array.isArray(value) || value.length > 10_000) throw new Error("change queue was invalid or unbounded.");
  const seen = new Set<string>();
  return Object.freeze(value.map((candidate, index) => {
    const item = record(candidate, `change queue ${index + 1}`);
    exactKeys(item, ["code", "description", "detail", "path", "status"], `change queue ${index + 1}`);
    if (!(typeof item.code === "string" && Object.hasOwn(CHANGE_STATUS, item.code))) {
      throw new Error(`change queue ${index + 1} status code was unknown.`);
    }
    const code = item.code as WorkspaceNativeChange["code"];
    if (item.status !== CHANGE_STATUS[code]) throw new Error(`change queue ${index + 1} status did not match its code.`);
    const path = text(item.path, `change queue ${index + 1} path`, 4_096);
    if (seen.has(path)) throw new Error("change queue repeated a path.");
    seen.add(path);
    return Object.freeze({
      path,
      description: text(item.description, `change queue ${index + 1} description`, 512),
      detail: text(item.detail, `change queue ${index + 1} detail`, 4_096, true),
      code,
      status: CHANGE_STATUS[code],
    });
  }));
}

function actions(value: unknown): readonly WorkspaceWorkAction[] {
  if (!Array.isArray(value) || value.length !== ACTION_IDS.length) {
    throw new Error("Workspace work actions omitted an established control.");
  }
  const seen = new Set<string>();
  const result = value.map((candidate, index) => {
    const item = record(candidate, `workspace work action ${index + 1}`);
    exactKeys(item, ["enabled", "id", "label"], `workspace work action ${index + 1}`);
    if (!ACTION_IDS.includes(item.id as WorkspaceWorkActionId) || seen.has(item.id as string)) {
      throw new Error("A workspace work action was unknown or duplicated.");
    }
    seen.add(item.id as string);
    return Object.freeze({
      id: item.id as WorkspaceWorkActionId,
      label: text(item.label, `workspace work action ${item.id as string}`, 96),
      enabled: bool(item.enabled, `workspace work action ${item.id as string} authority`),
    });
  });
  return Object.freeze(result);
}

function structural(value: unknown): WorkspaceFilesChangesModel["changes"]["structural"] {
  if (value === null) return null;
  const item = record(value, "structural change controls");
  exactKeys(item, ["canChoose", "hint", "missingSource", "missingSources", "moveTarget", "moveTargets"], "structural change controls");
  return Object.freeze({
    missingSources: choices(item.missingSources, "missing sources"),
    missingSource: text(item.missingSource, "missing source", 4_096, true),
    moveTargets: choices(item.moveTargets, "move targets"),
    moveTarget: text(item.moveTarget, "move target", 4_096, true),
    canChoose: bool(item.canChoose, "structural choice authority"),
    hint: text(item.hint, "structural change hint", 2_048),
  });
}

export function workspaceFilesChangesEnvelope(value: unknown, previousGeneration: number): Readonly<{
  generation: number;
  model: WorkspaceFilesChangesModel;
}> {
  const envelope = record(value, "workspace files and changes envelope");
  exactKeys(envelope, ["generation", "workbench"], "workspace files and changes envelope");
  if (!Number.isSafeInteger(envelope.generation) || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace files and changes generation was stale or invalid.");
  }
  const workbench = record(envelope.workbench, "workspace files and changes workbench");
  exactKeys(workbench, ["actions", "changes", "files"], "workspace files and changes workbench");
  const files = record(workbench.files, "workspace files");
  exactKeys(files, [
    "canEditMovePath", "canEditNewPath", "canSelectEntry", "entries", "movePath", "newPath",
    "selectedEntry", "status", "workspaceLabel", "workspaceRoot", "workspaceState",
  ], "workspace files");
  const changes = record(workbench.changes, "workspace changes");
  exactKeys(changes, [
    "autoSaveChecked", "autoSaveEnabled", "autoSaveHint", "baselineAvailable", "baselineText", "canEditText", "canSelectFile",
    "editState", "editVersion", "editorKind", "editorText", "files", "queue", "queueSummary", "scanState", "selectedFile", "structural",
  ], "workspace changes");
  if (!['none', 'text', 'binary'].includes(changes.editorKind as string)) throw new Error("The workspace editor kind was unknown.");
  if (!['idle', 'scanning', 'clean', 'changes', 'error'].includes(changes.scanState as string)) throw new Error("The workspace scan state was unknown.");
  if (files.workspaceState !== "current" && files.workspaceState !== "agent-assigned") {
    throw new Error("The workspace explorer state was unknown.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      files: Object.freeze({
        workspaceLabel: text(files.workspaceLabel, "workspace label", 512),
        workspaceRoot: text(files.workspaceRoot, "workspace root", 4_096),
        workspaceState: files.workspaceState,
        entries: entries(files.entries),
        newPath: text(files.newPath, "new path", 4_096, true),
        selectedEntry: text(files.selectedEntry, "selected entry", 4_096, true),
        movePath: text(files.movePath, "move path", 4_096, true),
        canEditNewPath: bool(files.canEditNewPath, "new path authority"),
        canSelectEntry: bool(files.canSelectEntry, "entry selection authority"),
        canEditMovePath: bool(files.canEditMovePath, "move path authority"),
        status: text(files.status, "management status", 2_048),
      }),
      changes: Object.freeze({
        files: choices(changes.files, "editor files"),
        selectedFile: text(changes.selectedFile, "selected file", 4_096, true),
        canSelectFile: bool(changes.canSelectFile, "file selection authority"),
        editorKind: changes.editorKind as WorkspaceFilesChangesModel["changes"]["editorKind"],
        editorText: text(changes.editorText, "editor text", 1_048_576, true),
        baselineText: text(changes.baselineText, "editor baseline", 1_048_576, true),
        baselineAvailable: bool(changes.baselineAvailable, "editor baseline availability"),
        canEditText: bool(changes.canEditText, "editor authority"),
        editState: text(changes.editState, "editor state", 512),
        editVersion: text(changes.editVersion, "editor version", 512, true),
        scanState: changes.scanState as WorkspaceFilesChangesModel["changes"]["scanState"],
        queueSummary: text(changes.queueSummary, "change queue summary", 512),
        queue: nativeChanges(changes.queue),
        autoSaveChecked: bool(changes.autoSaveChecked, "automatic save choice"),
        autoSaveEnabled: bool(changes.autoSaveEnabled, "automatic save authority"),
        autoSaveHint: text(changes.autoSaveHint, "automatic save hint", 2_048),
        structural: structural(changes.structural),
      }),
      actions: actions(workbench.actions),
    }),
  });
}

export function workspaceFilesChangesIntent(value: unknown): WorkspaceFilesChangesIntent {
  const intent = record(value, "workspace files and changes intent");
  if (intent.type === "activate") {
    if (!ACTION_IDS.includes(intent.action as WorkspaceWorkActionId)) throw new Error("The workspace work action was not recognized.");
    if (intent.action === "record-structural-change") {
      exactKeys(intent, ["action", "missingSource", "moveTarget", "type"], "workspace files and changes intent");
      return Object.freeze({
        type: "activate",
        action: "record-structural-change",
        missingSource: text(intent.missingSource, "structural change source", 4_096),
        moveTarget: text(intent.moveTarget, "structural change target", 4_096, true),
      });
    }
    if (Object.hasOwn(intent, "field") || Object.hasOwn(intent, "value")) {
      exactKeys(intent, ["action", "field", "type", "value"], "workspace files and changes intent");
      const action = intent.action as WorkspaceWorkActionId;
      const field = ACTION_ECHO_FIELDS[action];
      if (!field || intent.field !== field) throw new Error("The workspace work action field echo was not recognized.");
      return Object.freeze({
        type: "activate",
        action,
        field,
        value: text(intent.value, "workspace work action field echo", field === "editorText" ? 1_048_576 : 4_096, true),
      }) as WorkspaceFilesChangesIntent;
    }
    exactKeys(intent, ["action", "type"], "workspace files and changes intent");
    return Object.freeze({ type: "activate", action: intent.action as WorkspaceWorkActionId });
  }
  if (intent.type === "set-auto-save") {
    exactKeys(intent, ["checked", "type"], "workspace files and changes intent");
    return Object.freeze({ type: "set-auto-save", checked: bool(intent.checked, "automatic save choice") });
  }
  exactKeys(intent, ["field", "type", "value"], "workspace files and changes intent");
  if (intent.type !== "set-field" || !FIELD_IDS.includes(intent.field as WorkspaceWorkField)) {
    throw new Error("The workspace work field was not recognized.");
  }
  return Object.freeze({
    type: "set-field",
    field: intent.field as WorkspaceWorkField,
    value: text(
      intent.value,
      "workspace work field value",
      intent.field === "editorText" ? 1_048_576 : 4_096,
      true,
    ),
  });
}

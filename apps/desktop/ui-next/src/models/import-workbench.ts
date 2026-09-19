export type ImportWorkbenchModel = Readonly<{
  phase: "select" | "review";
  sourcePath: string;
  fileCount: string;
  folderCount: string;
  byteCount: string;
  summary: string;
  scopeNote: string;
  files: readonly string[];
  destinationPath: string;
  confirmLabel: string;
  busy: boolean;
  canChoose: boolean;
  canPreviewPath: boolean;
  canEditDestination: boolean;
  canChooseDestination: boolean;
  canConfirm: boolean;
}>;

export type ImportWorkbenchIntent =
  | Readonly<{ type: "choose-folder" }>
  | Readonly<{ type: "source-draft"; path: string }>
  | Readonly<{ type: "preview-path"; path: string }>
  | Readonly<{ type: "destination-draft"; path: string }>
  | Readonly<{ type: "choose-destination" }>
  | Readonly<{ type: "confirm-import" }>;

type JsonRecord = Record<string, unknown>;

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

function safeText(value: unknown, label: string, maximum: number, allowEmpty = false): string {
  if (typeof value !== "string" || (!allowEmpty && value.length === 0) || value.length > maximum
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

export function importWorkbenchEnvelope(
  value: unknown,
  previousGeneration: number,
): Readonly<{ generation: number; model: ImportWorkbenchModel }> {
  const envelope = record(value, "import workbench envelope");
  exactKeys(envelope, ["generation", "import"], "import workbench envelope");
  if (!Number.isSafeInteger(envelope.generation)
    || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The import workbench generation was stale or invalid.");
  }
  const candidate = record(envelope.import, "import workbench");
  exactKeys(candidate, [
    "byteCount",
    "busy",
    "canChoose",
    "canChooseDestination",
    "canConfirm",
    "canEditDestination",
    "canPreviewPath",
    "confirmLabel",
    "destinationPath",
    "fileCount",
    "files",
    "folderCount",
    "phase",
    "scopeNote",
    "sourcePath",
    "summary",
  ], "import workbench");
  if (!(["select", "review"] as const).includes(candidate.phase as "select" | "review")) {
    throw new Error("The import workbench phase was invalid.");
  }
  if (!Array.isArray(candidate.files) || candidate.files.length > 25) {
    throw new Error("The import workbench file list was invalid or unbounded.");
  }
  const files = Object.freeze(candidate.files.map((line, index) => (
    safeText(line, `import file ${index + 1}`, 4_096)
  )));
  if ((candidate.phase === "select" && (
    candidate.summary !== ""
    || files.length !== 0
    || candidate.destinationPath !== ""
    || candidate.busy !== false
    || candidate.canEditDestination !== false
    || candidate.canChooseDestination !== false
  ))
    || (candidate.phase === "review" && (
      candidate.sourcePath === ""
      || (candidate.busy === true && (
        candidate.canChoose !== false
        || candidate.canPreviewPath !== false
        || candidate.canEditDestination !== false
        || candidate.canChooseDestination !== false
        || candidate.canConfirm !== false
      ))
    ))) {
    throw new Error("The import workbench phase contradicted its preview facts.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      phase: candidate.phase as ImportWorkbenchModel["phase"],
      sourcePath: safeText(candidate.sourcePath, "import source path", 4_096, true),
      fileCount: safeText(candidate.fileCount, "import file count", 80),
      folderCount: safeText(candidate.folderCount, "import folder count", 80),
      byteCount: safeText(candidate.byteCount, "import byte count", 80),
      summary: safeText(candidate.summary, "import summary", 512, true),
      scopeNote: safeText(candidate.scopeNote, "import scope note", 2_048),
      files,
      destinationPath: safeText(candidate.destinationPath, "import destination path", 4_096, true),
      confirmLabel: safeText(candidate.confirmLabel, "import confirmation label", 96),
      busy: boolean(candidate.busy, "import busy state"),
      canChoose: boolean(candidate.canChoose, "choose-folder authority"),
      canPreviewPath: boolean(candidate.canPreviewPath, "typed-path authority"),
      canEditDestination: boolean(candidate.canEditDestination, "destination-draft authority"),
      canChooseDestination: boolean(candidate.canChooseDestination, "destination-chooser authority"),
      canConfirm: boolean(candidate.canConfirm, "import-confirmation authority"),
    }),
  });
}

export function importWorkbenchIntent(value: unknown): ImportWorkbenchIntent {
  const intent = record(value, "import workbench intent");
  if (intent.type === "source-draft"
    || intent.type === "preview-path"
    || intent.type === "destination-draft") {
    exactKeys(intent, ["path", "type"], "import workbench intent");
    const type = intent.type;
    return Object.freeze({
      type,
      path: safeText(
        intent.path,
        "import path",
        4_096,
        type === "source-draft" || type === "destination-draft",
      ),
    });
  }
  exactKeys(intent, ["type"], "import workbench intent");
  if (intent.type !== "choose-folder"
    && intent.type !== "choose-destination"
    && intent.type !== "confirm-import") {
    throw new Error("The import workbench intent was not recognized.");
  }
  return Object.freeze({ type: intent.type });
}

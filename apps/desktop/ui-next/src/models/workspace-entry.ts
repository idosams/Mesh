export type WorkspaceEntryMode = "empty" | "needs-import" | "ready";

export type RecentWorkspace = Readonly<{
  path: string;
  label: string;
  state: "available" | "current" | "agent-assigned" | "unavailable";
}>;

export type WorkspaceEntryModel = Readonly<{
  mode: WorkspaceEntryMode;
  eyebrow: string;
  title: string;
  description: string;
  disclosureLabel: string;
  disclosureOpen: boolean;
  chooseLabel: string;
  canChoose: boolean;
  canChooseManaged: boolean;
  retryLabel: string;
  canRetry: boolean;
  openPath: string;
  canEditPath: boolean;
  canOpenPath: boolean;
  recents: readonly RecentWorkspace[];
  selectedRecentPath: string;
  canSelectRecent: boolean;
  recentHint: string;
  recentOpenLabel: string;
  canOpenRecent: boolean;
  canForgetRecent: boolean;
  forgetRecentTitle: string;
}>;

export type WorkspaceEntryIntent =
  | Readonly<{ type: "choose-folder" }>
  | Readonly<{ type: "choose-managed-folder" }>
  | Readonly<{ type: "retry" }>
  | Readonly<{ type: "set-disclosure"; open: boolean }>
  | Readonly<{ type: "update-managed-path"; path: string }>
  | Readonly<{ type: "open-managed-path"; path: string }>
  | Readonly<{ type: "select-recent"; path: string }>
  | Readonly<{ type: "open-recent"; path: string }>
  | Readonly<{ type: "forget-recent"; path: string }>;

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
  if (actual.length !== canonical.length || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

const unsafeText = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;

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

function recentWorkspace(value: unknown): RecentWorkspace {
  const item = record(value, "recent workspace");
  exactKeys(item, ["label", "path", "state"], "recent workspace");
  if (!["available", "current", "agent-assigned", "unavailable"].includes(item.state as string)) {
    throw new Error("The recent workspace state was invalid.");
  }
  return Object.freeze({
    path: safeText(item.path, "recent workspace path", 4_096),
    label: safeText(item.label, "recent workspace label", 512),
    state: item.state as RecentWorkspace["state"],
  });
}

export function workspaceEntryEnvelope(value: unknown, previousGeneration: number): Readonly<{
  generation: number;
  model: WorkspaceEntryModel;
}> {
  const envelope = record(value, "workspace entry envelope");
  exactKeys(envelope, ["entry", "generation"], "workspace entry envelope");
  if (!Number.isSafeInteger(envelope.generation) || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace entry generation was stale or invalid.");
  }
  const entry = record(envelope.entry, "workspace entry");
  exactKeys(entry, [
    "canChoose", "canChooseManaged", "canEditPath", "canForgetRecent", "canOpenPath",
    "canOpenRecent", "canSelectRecent", "chooseLabel", "description", "disclosureLabel", "disclosureOpen",
    "eyebrow", "forgetRecentTitle", "mode", "openPath", "recentHint", "recentOpenLabel",
    "recents", "retryLabel", "canRetry", "selectedRecentPath", "title",
  ], "workspace entry");
  if (!["empty", "needs-import", "ready"].includes(entry.mode as string)) {
    throw new Error("The workspace entry mode was invalid.");
  }
  if (!Array.isArray(entry.recents) || entry.recents.length > 256) {
    throw new Error("The recent workspace list was invalid or unbounded.");
  }
  const recents = Object.freeze(entry.recents.map(recentWorkspace));
  const selectedRecentPath = safeText(entry.selectedRecentPath, "selected recent path", 4_096, true);
  if (selectedRecentPath && !recents.some((item) => item.path === selectedRecentPath)) {
    throw new Error("The selected recent workspace was not in the projected list.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      mode: entry.mode as WorkspaceEntryMode,
      eyebrow: safeText(entry.eyebrow, "workspace entry eyebrow", 80),
      title: safeText(entry.title, "workspace entry title", 240),
      description: safeText(entry.description, "workspace entry description", 2_000),
      disclosureLabel: safeText(entry.disclosureLabel, "workspace entry disclosure label", 240),
      disclosureOpen: boolean(entry.disclosureOpen, "workspace entry disclosure state"),
      chooseLabel: safeText(entry.chooseLabel, "workspace entry choose label", 120),
      canChoose: boolean(entry.canChoose, "workspace entry choose availability"),
      canChooseManaged: boolean(entry.canChooseManaged, "managed picker availability"),
      retryLabel: safeText(entry.retryLabel, "workspace refresh label", 120),
      canRetry: boolean(entry.canRetry, "workspace refresh availability"),
      openPath: safeText(entry.openPath, "managed workspace path", 4_096, true),
      canEditPath: boolean(entry.canEditPath, "managed path edit availability"),
      canOpenPath: boolean(entry.canOpenPath, "managed path open availability"),
      recents,
      selectedRecentPath,
      canSelectRecent: boolean(entry.canSelectRecent, "recent workspace selection availability"),
      recentHint: safeText(entry.recentHint, "recent workspace hint", 1_000),
      recentOpenLabel: safeText(entry.recentOpenLabel, "recent workspace action", 120),
      canOpenRecent: boolean(entry.canOpenRecent, "recent workspace open availability"),
      canForgetRecent: boolean(entry.canForgetRecent, "recent workspace forget availability"),
      forgetRecentTitle: safeText(entry.forgetRecentTitle, "recent workspace forget explanation", 1_000, true),
    }),
  });
}

export function workspaceEntryIntent(value: unknown): WorkspaceEntryIntent {
  const intent = record(value, "workspace entry intent");
  if (["choose-folder", "choose-managed-folder", "retry"].includes(intent.type as string)) {
    exactKeys(intent, ["type"], "workspace entry intent");
    return Object.freeze({ type: intent.type }) as WorkspaceEntryIntent;
  }
  if (intent.type === "set-disclosure") {
    exactKeys(intent, ["open", "type"], "workspace entry intent");
    return Object.freeze({ type: intent.type, open: boolean(intent.open, "workspace entry disclosure state") });
  }
  if (["update-managed-path", "open-managed-path", "select-recent", "open-recent", "forget-recent"].includes(intent.type as string)) {
    exactKeys(intent, ["path", "type"], "workspace entry intent");
    return Object.freeze({
      type: intent.type,
      path: safeText(intent.path, "workspace entry intent path", 4_096, intent.type === "update-managed-path"),
    }) as WorkspaceEntryIntent;
  }
  throw new Error("The workspace entry intent was not recognized.");
}

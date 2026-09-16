export type WorkspaceChromeModel = Readonly<{
  serviceState: "starting" | "ready" | "attention";
  serviceLabel: string;
  workspaceReady: boolean;
  nativeChangeCount: number;
}>;

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

function safeText(value: unknown, label: string, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

export function workspaceChromeEnvelope(value: unknown, previousGeneration: number): Readonly<{
  generation: number;
  workspaceIdentity: number;
  model: WorkspaceChromeModel;
}> {
  const envelope = record(value, "workspace chrome envelope");
  exactKeys(envelope, ["chrome", "generation", "workspaceIdentity"], "workspace chrome envelope");
  if (!Number.isSafeInteger(envelope.generation) || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace chrome generation was stale or invalid.");
  }
  if (!Number.isSafeInteger(envelope.workspaceIdentity) || (envelope.workspaceIdentity as number) < 0) {
    throw new Error("The workspace chrome identity was invalid.");
  }
  const chrome = record(envelope.chrome, "workspace chrome");
  exactKeys(chrome, ["nativeChangeCount", "serviceLabel", "serviceState", "workspaceReady"], "workspace chrome");
  if (!["starting", "ready", "attention"].includes(chrome.serviceState as string)) {
    throw new Error("The workspace chrome service state was invalid.");
  }
  if (typeof chrome.workspaceReady !== "boolean") {
    throw new Error("The workspace chrome readiness was not boolean.");
  }
  if (!Number.isSafeInteger(chrome.nativeChangeCount)
    || (chrome.nativeChangeCount as number) < 0
    || (chrome.nativeChangeCount as number) > 1_000_000) {
    throw new Error("The workspace chrome change count was invalid or unbounded.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    workspaceIdentity: envelope.workspaceIdentity as number,
    model: Object.freeze({
      serviceState: chrome.serviceState as WorkspaceChromeModel["serviceState"],
      serviceLabel: safeText(chrome.serviceLabel, "service label", 160),
      workspaceReady: chrome.workspaceReady,
      nativeChangeCount: chrome.nativeChangeCount as number,
    }),
  });
}

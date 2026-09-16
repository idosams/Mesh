export type ConfirmationModel = Readonly<{
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  tone: "standard" | "destructive";
}>;

export type ConfirmationIntent = Readonly<{ type: "confirm" | "cancel" }>;

export type ConfirmationKeyboardAction = "cancel" | "focus-cancel" | "focus-confirm" | null;

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

function safeText(value: unknown, label: string, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

export function confirmationEnvelope(
  value: unknown,
  previousGeneration: number,
): Readonly<{ generation: number; model: ConfirmationModel }> {
  const envelope = record(value, "confirmation envelope");
  exactKeys(envelope, ["confirmation", "generation"], "confirmation envelope");
  if (!Number.isSafeInteger(envelope.generation)
    || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The confirmation generation was stale or invalid.");
  }
  const candidate = record(envelope.confirmation, "confirmation");
  exactKeys(candidate, [
    "cancelLabel",
    "confirmLabel",
    "description",
    "title",
    "tone",
  ], "confirmation");
  if (candidate.tone !== "standard" && candidate.tone !== "destructive") {
    throw new Error("The confirmation tone was invalid.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      title: safeText(candidate.title, "confirmation title", 160),
      description: safeText(candidate.description, "confirmation description", 16_384),
      confirmLabel: safeText(candidate.confirmLabel, "confirmation action label", 96),
      cancelLabel: safeText(candidate.cancelLabel, "confirmation cancel label", 96),
      tone: candidate.tone,
    }),
  });
}

export function confirmationIntent(value: unknown): ConfirmationIntent {
  const intent = record(value, "confirmation intent");
  exactKeys(intent, ["type"], "confirmation intent");
  if (intent.type !== "confirm" && intent.type !== "cancel") {
    throw new Error("The confirmation intent was not recognized.");
  }
  return Object.freeze({ type: intent.type });
}

export function confirmationKeyboardAction(
  key: string,
  shiftKey: boolean,
  atCancel: boolean,
  atConfirm: boolean,
): ConfirmationKeyboardAction {
  if (key === "Escape") return "cancel";
  if (key !== "Tab") return null;
  if (shiftKey && atCancel) return "focus-confirm";
  if (!shiftKey && atConfirm) return "focus-cancel";
  return null;
}

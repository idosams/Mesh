import { BOUNDED_CHOICE_LIMIT, boundedChoiceProjection, type BoundedChoiceProjection } from "./choice-projection";

export const WORKSPACE_DESTINATION_ACTION_IDS = Object.freeze([
  "choose-destination",
  "preview-single",
  "confirm-single",
  "preview-all",
  "confirm-batch",
] as const);

export type WorkspaceDestinationActionId = typeof WORKSPACE_DESTINATION_ACTION_IDS[number];
export type WorkspaceDestinationPlanState = "ready" | "information" | "blocked" | "complete";

export type WorkspaceDestinationChoice = Readonly<{ value: string; label: string }>;
export type WorkspaceDestinationChoiceProjection = BoundedChoiceProjection<WorkspaceDestinationChoice>;

export function workspaceDestinationChoiceProjection(
  choices: readonly WorkspaceDestinationChoice[],
  filterText: string,
  selectedValue: string,
  maximum = BOUNDED_CHOICE_LIMIT,
): WorkspaceDestinationChoiceProjection {
  return boundedChoiceProjection(
    choices,
    filterText,
    selectedValue,
    (choice) => choice.value,
    (choice) => [choice.value, choice.label],
    maximum,
  );
}
export type WorkspaceDestinationAction = Readonly<{
  id: WorkspaceDestinationActionId;
  label: string;
  enabled: boolean;
}>;
export type WorkspaceDestinationPlan = Readonly<{
  state: WorkspaceDestinationPlanState;
  text: string;
}>;
export type WorkspaceDestinationModel = Readonly<{
  files: readonly WorkspaceDestinationChoice[];
  selectedFile: string;
  destination: string;
  chooserRevision: number;
  canSelectFile: boolean;
  canEditDestination: boolean;
  hint: string;
  plan: WorkspaceDestinationPlan | null;
  actions: readonly WorkspaceDestinationAction[];
}>;

export type WorkspaceDestinationIntent =
  | Readonly<{ type: "set-field"; field: "selectedFile" | "destination"; value: string }>
  | Readonly<{
      type: "activate";
      action: Exclude<WorkspaceDestinationActionId, "preview-single" | "preview-all">;
    }>
  | Readonly<{
      type: "activate";
      action: "preview-single";
      selectedFile: string;
      destination: string;
    }>
  | Readonly<{
      type: "activate";
      action: "preview-all";
      destination: string;
    }>;

type JsonRecord = Record<string, unknown>;
const PLAN_STATES: readonly WorkspaceDestinationPlanState[] = Object.freeze(["ready", "information", "blocked", "complete"]);
const unsafeText = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;

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

function safeText(value: unknown, label: string, maximum: number, empty = false): string {
  if (typeof value !== "string" || (!empty && value.length === 0) || value.length > maximum || unsafeText.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function safeSingleLine(value: unknown, label: string, maximum: number, empty = false): string {
  const text = safeText(value, label, maximum, empty);
  if (/[\r\n]/u.test(text)) throw new Error(`${label} was unsafe or unbounded.`);
  return text;
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new Error(`${label} was not boolean.`);
  return value;
}

function choice(value: unknown): WorkspaceDestinationChoice {
  const candidate = record(value, "workspace destination file choice");
  exactKeys(candidate, ["label", "value"], "workspace destination file choice");
  return Object.freeze({
    value: safeSingleLine(candidate.value, "workspace destination file value", 4_096),
    label: safeSingleLine(candidate.label, "workspace destination file label", 4_608),
  });
}

function action(value: unknown): WorkspaceDestinationAction {
  const candidate = record(value, "workspace destination action");
  exactKeys(candidate, ["enabled", "id", "label"], "workspace destination action");
  if (!WORKSPACE_DESTINATION_ACTION_IDS.includes(candidate.id as WorkspaceDestinationActionId)) {
    throw new Error("The workspace destination action was invalid.");
  }
  return Object.freeze({
    id: candidate.id as WorkspaceDestinationActionId,
    label: safeSingleLine(candidate.label, "workspace destination action label", 160),
    enabled: boolean(candidate.enabled, "workspace destination action availability"),
  });
}

function plan(value: unknown): WorkspaceDestinationPlan | null {
  if (value === null) return null;
  const candidate = record(value, "workspace destination plan");
  exactKeys(candidate, ["state", "text"], "workspace destination plan");
  if (!PLAN_STATES.includes(candidate.state as WorkspaceDestinationPlanState)) {
    throw new Error("The workspace destination plan state was invalid.");
  }
  return Object.freeze({
    state: candidate.state as WorkspaceDestinationPlanState,
    text: safeText(candidate.text, "workspace destination plan text", 262_144),
  });
}

export function workspaceDestinationEnvelope(value: unknown, previousGeneration: number): Readonly<{
  generation: number;
  model: WorkspaceDestinationModel;
}> {
  const envelope = record(value, "workspace destination envelope");
  exactKeys(envelope, ["destination", "generation"], "workspace destination envelope");
  if (!Number.isSafeInteger(envelope.generation) || (envelope.generation as number) <= previousGeneration) {
    throw new Error("The workspace destination generation was stale or invalid.");
  }
  const candidate = record(envelope.destination, "workspace destination");
  exactKeys(candidate, ["actions", "canEditDestination", "canSelectFile", "chooserRevision", "destination", "files", "hint", "plan", "selectedFile"], "workspace destination");
  if (!Array.isArray(candidate.files) || candidate.files.length > 4_096 || !Array.isArray(candidate.actions)) {
    throw new Error("The workspace destination choices or actions were invalid or unbounded.");
  }
  const files = Object.freeze(candidate.files.map(choice));
  const actions = Object.freeze(candidate.actions.map(action));
  if (new Set(files.map((item) => item.value)).size !== files.length) {
    throw new Error("The workspace destination file choices were not unique.");
  }
  if (actions.length !== WORKSPACE_DESTINATION_ACTION_IDS.length
    || new Set(actions.map((item) => item.id)).size !== actions.length
    || WORKSPACE_DESTINATION_ACTION_IDS.some((id) => !actions.some((item) => item.id === id))) {
    throw new Error("The workspace destination projection omitted an established control.");
  }
  const selectedFile = safeSingleLine(candidate.selectedFile, "selected workspace destination file", 4_096, true);
  if (selectedFile && !files.some((item) => item.value === selectedFile)) {
    throw new Error("The selected workspace destination file was not in the projected choices.");
  }
  if (!Number.isSafeInteger(candidate.chooserRevision) || (candidate.chooserRevision as number) < 0) {
    throw new Error("The workspace destination chooser revision was invalid.");
  }
  return Object.freeze({
    generation: envelope.generation as number,
    model: Object.freeze({
      files,
      selectedFile,
      destination: safeSingleLine(candidate.destination, "workspace destination path", 4_096, true),
      chooserRevision: candidate.chooserRevision as number,
      canSelectFile: boolean(candidate.canSelectFile, "workspace destination file selection availability"),
      canEditDestination: boolean(candidate.canEditDestination, "workspace destination path availability"),
      hint: safeSingleLine(candidate.hint, "workspace destination hint", 2_000),
      plan: plan(candidate.plan),
      actions,
    }),
  });
}

export function workspaceDestinationIntent(value: unknown): WorkspaceDestinationIntent {
  const intent = record(value, "workspace destination intent");
  if (intent.type === "activate") {
    if (!WORKSPACE_DESTINATION_ACTION_IDS.includes(intent.action as WorkspaceDestinationActionId)) {
      throw new Error("The workspace destination intent was not recognized.");
    }
    if (intent.action === "preview-single") {
      exactKeys(intent, ["action", "destination", "selectedFile", "type"], "workspace destination intent");
      return Object.freeze({
        type: "activate",
        action: "preview-single",
        selectedFile: safeSingleLine(intent.selectedFile, "workspace destination selected file", 4_096),
        destination: safeSingleLine(intent.destination, "workspace destination path", 4_096),
      });
    }
    if (intent.action === "preview-all") {
      exactKeys(intent, ["action", "destination", "type"], "workspace destination intent");
      return Object.freeze({
        type: "activate",
        action: "preview-all",
        destination: safeSingleLine(intent.destination, "workspace destination path", 4_096),
      });
    }
    exactKeys(intent, ["action", "type"], "workspace destination intent");
    return Object.freeze({
      type: "activate",
      action: intent.action as Exclude<WorkspaceDestinationActionId, "preview-single" | "preview-all">,
    });
  }
  if (intent.type === "set-field") {
    exactKeys(intent, ["field", "type", "value"], "workspace destination intent");
    if (intent.field !== "selectedFile" && intent.field !== "destination") {
      throw new Error("The workspace destination intent was not recognized.");
    }
    return Object.freeze({
      type: "set-field",
      field: intent.field,
      value: safeSingleLine(intent.value, "workspace destination intent value", 4_096, true),
    });
  }
  throw new Error("The workspace destination intent was not recognized.");
}

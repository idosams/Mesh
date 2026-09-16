export const ISLAND_MOUNT_TIMEOUT_MS = 1_500;

type TimerHandle = ReturnType<typeof setTimeout>;

export type IslandLiveness = Readonly<{
  begin: (generation: number, parts?: readonly string[]) => void;
  commit: (generation: number, part?: string) => boolean;
  cancel: (generation: number) => boolean;
  fail: (generation: number, error: unknown) => void;
}>;

export function createIslandLiveness({
  reject,
  schedule = setTimeout,
  cancel = clearTimeout,
}: Readonly<{
  reject: (generation: number, reason: string) => void;
  schedule?: (callback: () => void, delay: number) => TimerHandle;
  cancel?: (handle: TimerHandle) => void;
}>): IslandLiveness {
  let pending: {
    generation: number;
    expectedParts: ReadonlySet<string>;
    committedParts: Set<string>;
    timer: TimerHandle;
  } | null = null;
  let activeGeneration: number | null = null;

  const cancelGeneration = (generation: number): boolean => {
    if (activeGeneration !== generation) return false;
    if (pending?.generation === generation) cancel(pending.timer);
    pending = null;
    activeGeneration = null;
    return true;
  };
  const rejectPending = (generation: number, reason: string) => {
    if (!cancelGeneration(generation)) return;
    reject(generation, reason);
  };

  return Object.freeze({
    begin: (generation, parts = ["root"]) => {
      if (!Number.isSafeInteger(generation) || generation < 0) {
        throw new Error("Island liveness requires a non-negative safe generation.");
      }
      const expectedParts = new Set(parts);
      if (expectedParts.size !== parts.length || expectedParts.size === 0
        || [...expectedParts].some((part) => !part)) {
        throw new Error("Island liveness requires unique named render parts.");
      }
      if (pending) cancel(pending.timer);
      activeGeneration = generation;
      const timer = schedule(
        () => rejectPending(generation, "The React surface did not commit in time."),
        ISLAND_MOUNT_TIMEOUT_MS,
      );
      pending = { generation, expectedParts, committedParts: new Set(), timer };
    },
    commit: (generation, part = "root") => {
      if (!pending || pending.generation !== generation || !pending.expectedParts.has(part)) return false;
      pending.committedParts.add(part);
      if (pending.committedParts.size !== pending.expectedParts.size) return false;
      cancel(pending.timer);
      pending = null;
      return activeGeneration === generation;
    },
    cancel: cancelGeneration,
    fail: (generation, error) => {
      const reason = error instanceof Error && error.message
        ? error.message
        : "The React surface failed before it became usable.";
      rejectPending(generation, reason);
    },
  });
}

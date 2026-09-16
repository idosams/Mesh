export type RovingSelectionMode = "vertical-clamp" | "all-wrap";

export function rovingSelectionIndex(
  length: number,
  current: number,
  key: string,
  mode: RovingSelectionMode,
): number | null {
  if (!Number.isSafeInteger(length) || length < 1
    || !Number.isSafeInteger(current) || current < 0 || current >= length) return null;
  if (key === "Home") return 0;
  if (key === "End") return length - 1;
  const forward = key === "ArrowDown" || (mode === "all-wrap" && key === "ArrowRight");
  const backward = key === "ArrowUp" || (mode === "all-wrap" && key === "ArrowLeft");
  if (!forward && !backward) return null;
  if (mode === "all-wrap") return (current + (forward ? 1 : -1) + length) % length;
  return forward ? Math.min(current + 1, length - 1) : Math.max(current - 1, 0);
}

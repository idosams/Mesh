type FocusOwner = Readonly<{ isConnected: boolean }>;
type ActiveFocusRoot = Readonly<{ activeElement: FocusOwner | null }>;
type FocusEventRoot = Pick<Document, "addEventListener" | "removeEventListener">;

export type ImportFocusRequest = Readonly<{
  generation: number;
  owner: FocusOwner;
}>;

export type ImportReviewFocusAuthorization = Readonly<{
  consume: () => boolean;
  cancel: () => void;
}>;

export function captureImportFocusRequest(
  root: ActiveFocusRoot,
  generation: number,
): ImportFocusRequest | null {
  if (!Number.isSafeInteger(generation) || generation < 0 || !root.activeElement) return null;
  return Object.freeze({ generation, owner: root.activeElement });
}

export function advanceImportFocusRequest(
  request: ImportFocusRequest | null,
  previousGeneration: number,
  nextGeneration: number,
): ImportFocusRequest | null {
  if (!request || request.generation !== previousGeneration || !Number.isSafeInteger(nextGeneration)) return null;
  return Object.freeze({ generation: nextGeneration, owner: request.owner });
}

export function consumeImportReviewFocus(
  request: ImportFocusRequest | null,
  generation: number,
  root: ActiveFocusRoot,
): boolean {
  return Boolean(
    request
    && request.generation === generation
    && request.owner.isConnected
    && root.activeElement === request.owner,
  );
}

export function authorizeImportReviewFocus(
  request: ImportFocusRequest | null,
  generation: number,
  root: ActiveFocusRoot,
  events: FocusEventRoot,
): ImportReviewFocusAuthorization | null {
  if (!request || !consumeImportReviewFocus(request, generation, root)) return null;
  const owner = request.owner;
  let authorized = true;
  let listening = true;
  const stopListening = () => {
    if (!listening) return;
    listening = false;
    events.removeEventListener("focusin", handleFocusChange, true);
  };
  const handleFocusChange = () => {
    // WebKit can dispatch focusin while React is removing the initiating control but before
    // `isConnected` reflects that removal. Decide after the current DOM commit: a deliberate move
    // leaves the owner connected and revokes the handoff; the expected replacement does not.
    queueMicrotask(() => {
      if (owner.isConnected && root.activeElement !== owner) {
        authorized = false;
        stopListening();
      }
    });
  };
  events.addEventListener("focusin", handleFocusChange, true);
  return Object.freeze({
    consume: () => {
      const accepted = authorized;
      authorized = false;
      stopListening();
      return accepted;
    },
    cancel: () => {
      authorized = false;
      stopListening();
    },
  });
}

const NOTICE_SCHEMA = 'mesh.notice/v1';
const NOTICE_MESSAGE_LIMIT = 4_096;
const NOTICE_PROOFS = Object.freeze(['agent-handoff-rescanned']);

function boundedMessage(value) {
  const message = String(value);
  if (message.length === 0) return 'Mesh status unavailable.';
  if (message.length <= NOTICE_MESSAGE_LIMIT) return message;
  const suffix = '… [notice shortened]';
  return `${message.slice(0, NOTICE_MESSAGE_LIMIT - suffix.length)}${suffix}`;
}

export function createNoticeSource(document, CustomEventConstructor = CustomEvent) {
  let generation = 0;
  let current = null;

  const project = (next) => {
    current = Object.freeze(next);
    document.dispatchEvent(new CustomEventConstructor('mesh:notice-projection', {
      detail: current,
    }));
    return current;
  };

  const nextGeneration = () => {
    if (generation >= Number.MAX_SAFE_INTEGER) {
      throw new Error('The notice sequence is exhausted');
    }
    generation += 1;
    return generation;
  };

  document.addEventListener('mesh:notice-snapshot-request', () => {
    if (current) {
      document.dispatchEvent(new CustomEventConstructor('mesh:notice-projection', {
        detail: current,
      }));
    }
  });

  return Object.freeze({
    show(message, error = false) {
      return project({
        schema: NOTICE_SCHEMA,
        generation: nextGeneration(),
        message: boundedMessage(message),
        error: Boolean(error),
        proof: null,
      });
    },
    snapshot() {
      return current;
    },
    clearProof() {
      if (!current || current.proof === null) return false;
      project({ ...current, generation: nextGeneration(), proof: null });
      return true;
    },
    markProof(expectedGeneration, proof) {
      if (!current
        || current.generation !== expectedGeneration
        || current.error
        || !NOTICE_PROOFS.includes(proof)) return false;
      project({ ...current, generation: nextGeneration(), proof });
      return true;
    },
  });
}

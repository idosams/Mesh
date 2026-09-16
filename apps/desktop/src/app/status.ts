// The six-state user model, as the only route from an internal state to a word on a screen.
//
// # The rule this file is
//
// Plan §3.4: user-facing status is exactly six words — Working, Saved privately, Available to
// team, Ready for review, Needs attention, Approved — and nine terms from the internal model may
// never reach a person. A rule that lives only in a document is a rule somebody breaks in a
// hurry, so it lives here as a lookup that has no other outcome: it answers with an approved
// wording, or it throws.
//
// # Why there is no fallback
//
// The obvious alternative — return the internal name, or default to one of the six — is the
// failure this file exists to prevent. A fallback is silent: it ships an unreviewed word to a
// person and nothing fails, so nobody finds out until a user reads it. Throwing is loud, and it
// is loud in the desktop build rather than in front of a user, because `status.test.ts` checks
// this mapping against §4.1 of `docs/product-prd.md` in both directions on every run. An internal
// state that gains a row in the product model and no wording here fails the build that day.
//
// The thrown error carries the unmapped name on a FIELD and not in its message. The name of a
// state this application has never heard of is precisely the string the vocabulary lint cannot
// check, so it never goes into a sentence.
//
// # Where the words come from
//
// `../strings/status.json`, which is one file for one reason: a catalogue the lint reads and a
// mapping the application reads would drift, and the drift would be invisible. That file is
// checked by `tools/program/vocab-lint` — every value at `$.states.*.status` against the six
// words, every string in it against the nine terms — and by the tests beside this file against
// the product requirements. Nothing here re-states a word; it only reads them.

import catalog from '../strings/status.json' with { type: 'json' };
import { ERROR_COPY } from '../strings/errors.ts';

/** One of the six words this application may show for the state of a person's work. */
export type UserStatus = string;

/** What a person reads for one of the six states. */
export type UserStateEntry = {
  /** The identifier the interface uses. */
  readonly id: string;
  /** The status word itself — one of the six, in the product's vocabulary. */
  readonly status: UserStatus;
  /** What the status tells a person. */
  readonly means: string;
  /** What a person can do about it. */
  readonly nextStep: string;
};

/** One internal state, and the only wording this application may show for it. */
export type StateWording = {
  /** The internal state, as `docs/product-prd.md` §4.1 names it. */
  readonly internalState: string;
  /** The user-facing wording. Four of these are status words; four name a thing. */
  readonly wording: string;
};

/** The six states, in the order the product requirements present them. */
export const USER_STATES: readonly UserStateEntry[] = Object.freeze(
  catalog.states.map((state) => Object.freeze({ ...state })),
);

/** Just the six words, in that same order. */
export const USER_STATUSES: readonly UserStatus[] = Object.freeze(
  USER_STATES.map((state) => state.status),
);

/** Every internal state this application can put words to. */
export const STATE_WORDINGS: readonly StateWording[] = Object.freeze(
  catalog.internalStates.map((entry) => Object.freeze({ ...entry })),
);

/** The names of those internal states — the closed set this application accepts. */
export const INTERNAL_STATES: readonly string[] = Object.freeze(
  STATE_WORDINGS.map((entry) => entry.internalState),
);

/**
 * An internal state arrived that has no approved wording.
 *
 * The message is the reviewed sentence from `../strings/errors.ts` and says nothing about which
 * state it was; `internalState` carries that for a report. Rendering `error.message` is therefore
 * safe by construction, which is the property a fallback string cannot have.
 */
export class UnmappedInternalState extends Error {
  /** The state that has no wording. For a report — never for a screen. */
  readonly internalState: string;

  constructor(internalState: string) {
    super(ERROR_COPY.unmappedInternalState());
    this.name = 'UnmappedInternalState';
    this.internalState = internalState;
  }
}

/** The six-state entry with this identifier, when there is one. */
export const stateById = (id: string): UserStateEntry | undefined =>
  USER_STATES.find((state) => state.id === id);

/** The six-state entry carrying this status word, when there is one. */
export const stateByStatus = (status: string): UserStateEntry | undefined =>
  USER_STATES.find((state) => state.status === status);

/** Is this one of the six words? */
export const isUserStatus = (value: string): boolean =>
  USER_STATUSES.includes(value);

/**
 * The only wording this application may show for `internalState`.
 *
 * @throws {UnmappedInternalState} when there is none. There is deliberately no other outcome:
 * no default word, no passing the internal name through, no empty string.
 */
export const wordingFor = (internalState: string): string => {
  const found = STATE_WORDINGS.find((entry) => entry.internalState === internalState);
  if (found === undefined) throw new UnmappedInternalState(internalState);
  return found.wording;
};

/**
 * The six-state entry for `internalState`, when its wording is one of the six.
 *
 * `undefined` where the wording names a thing rather than a state — *Shared version* and
 * *Earlier version* are wordings, not statuses, and pretending otherwise would put a seventh
 * meaning on one of the six. An internal state with no wording at all still throws.
 *
 * @throws {UnmappedInternalState} when `internalState` has no approved wording.
 */
export const statusFor = (internalState: string): UserStateEntry | undefined =>
  stateByStatus(wordingFor(internalState));

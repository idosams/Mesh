/** The rule registry. A surface declares which rules it opts into. */

import * as forbiddenWords from './forbidden-words.mjs';
import * as sixState from './six-state.mjs';
import * as requirements from './requirements.mjs';
import * as journey from './journey.mjs';
import * as mapping from './mapping.mjs';

export const RULES = Object.freeze([forbiddenWords, sixState, requirements, journey, mapping]);

export const RULE_IDS = Object.freeze(RULES.map((rule) => rule.id));

/** @returns {object|undefined} */
export function ruleById(id) {
  return RULES.find((rule) => rule.id === id);
}

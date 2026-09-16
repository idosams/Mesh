// Version pinning: what the baseline is, exactly, and whether it drifted.
//
// A baseline quoted by name is not a baseline. "Git" is not a comparator;
// `git version 2.51.0` on this host on this day is. This module reads the
// version out of the tool at run time and holds it against `versions.json`.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { attempt, onPath } from './exec.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));

/** The pin file, parsed. */
export function loadVersions() {
  return JSON.parse(readFileSync(join(HERE, '..', 'versions.json'), 'utf8'));
}

/** The pin entry for one baseline id, or `undefined`. */
export function pinFor(id) {
  return loadVersions().baselines.find((entry) => entry.id === id);
}

/**
 * Resolves a baseline's tool version and checks it against the pin.
 *
 * Three outcomes, all of them recorded rather than thrown:
 *   available + pinned   — publishable
 *   available + drifted  — refused unless exploratory; the row says so
 *   absent               — unsupported; no timing number is produced at all
 */
export function resolveTool(pin) {
  if (pin.version_command === null) {
    return {
      available: true,
      version: 'host',
      version_command: pin.version_source,
      pin_mode: pin.pin_mode,
      pin_accepted: null,
      pin_satisfied: true,
    };
  }
  const [program, ...args] = pin.version_command;
  if (!onPath(program)) {
    return {
      available: false,
      reason: `\`${program}\` is not on PATH on this host`,
      version: null,
      version_command: pin.version_source,
      pin_mode: pin.pin_mode,
      pin_accepted: pin.accepted,
      pin_satisfied: false,
    };
  }
  const probed = attempt(program, args);
  if (probed.code !== 0) {
    return {
      available: false,
      reason: `\`${pin.version_command.join(' ')}\` exited ${probed.code}`,
      version: null,
      version_command: pin.version_source,
      pin_mode: pin.pin_mode,
      pin_accepted: pin.accepted,
      pin_satisfied: false,
    };
  }
  const version = parseVersion(probed.stdout);
  return {
    available: true,
    version,
    version_command: pin.version_source,
    pin_mode: pin.pin_mode,
    pin_accepted: pin.accepted,
    pin_satisfied: Array.isArray(pin.accepted) ? pin.accepted.includes(version) : true,
  };
}

/**
 * The first dotted numeric run in a `--version` banner.
 *
 * `git version 2.51.0`, `jj 0.34.0`, `rsync  version 3.4.1  protocol version 32`
 * all reduce to the same shape. A banner with no version in it yields the
 * trimmed first line, which will then fail the pin loudly rather than silently
 * matching nothing.
 */
export function parseVersion(banner) {
  const firstLine = banner.split('\n')[0].trim();
  const match = firstLine.match(/\d+(?:\.\d+)+/);
  return match ? match[0] : firstLine;
}

/** The `baseline` block of a result row. */
export function baselineBlock(pin, tool) {
  return {
    id: pin.id,
    title: pin.title,
    tool: pin.tool,
    version: tool.version,
    version_source: tool.version_command,
    pin_mode: tool.pin_mode,
    pin_accepted: tool.pin_accepted,
    pin_satisfied: tool.pin_satisfied,
    terms: pin.terms,
  };
}

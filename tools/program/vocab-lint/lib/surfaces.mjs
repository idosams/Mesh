/**
 * The surface manifest: which files are user-facing, how their strings are
 * extracted, which rules they opt into, and how much suppression they may
 * carry.
 *
 * The manifest is validated at load — a typo in a rule name must fail loudly
 * rather than silently disable a check, which is the classic way a lint stops
 * linting.
 */

import fs from 'node:fs';
import path from 'node:path';
import { MODES, MODE_EXTENSIONS } from './extract.mjs';
import { RULE_IDS } from './rules/index.mjs';
import { parseStatusPath } from './rules/six-state.mjs';
import { matchFiles, globExtensions, globToRegExp } from './globs.mjs';

export function loadManifest(toolRoot) {
  const file = path.join(toolRoot, 'surfaces.json');
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (error) {
    throw new Error(`cannot read ${file}: ${error.message}`);
  }
  if (!Array.isArray(parsed?.surfaces) || parsed.surfaces.length === 0) {
    throw new Error(`${file} must declare a non-empty "surfaces" array`);
  }
  parsed.surfaces.forEach((surface, index) => validateSurface(surface, index, file));
  return parsed;
}

export function validateSurface(surface, index, file) {
  const where = `${file}: surfaces[${index}]`;
  if (typeof surface?.id !== 'string' || surface.id.length === 0) {
    throw new Error(`${where} needs a non-empty "id"`);
  }
  const hasPath = typeof surface.path === 'string' && surface.path.length > 0;
  const hasGlob = typeof surface.glob === 'string' && surface.glob.length > 0;
  if (hasPath === hasGlob) {
    throw new Error(`${where} (${surface.id}) needs exactly one of "path" or "glob"`);
  }
  if (!MODES.includes(surface.mode)) {
    throw new Error(`${where} (${surface.id}) has mode "${surface.mode}"; expected one of ${MODES.join(', ')}`);
  }
  if (!Array.isArray(surface.rules) || surface.rules.length === 0) {
    throw new Error(`${where} (${surface.id}) needs a non-empty "rules" array`);
  }
  for (const rule of surface.rules) {
    if (!RULE_IDS.includes(rule)) {
      throw new Error(`${where} (${surface.id}) names unknown rule "${rule}"; known rules: ${RULE_IDS.join(', ')}`);
    }
  }
  if (!Number.isInteger(surface.allowRegionBudget) || surface.allowRegionBudget < 0) {
    throw new Error(`${where} (${surface.id}) needs an integer "allowRegionBudget" of 0 or more`);
  }
  if (typeof surface.required !== 'boolean') {
    throw new Error(`${where} (${surface.id}) needs a boolean "required"`);
  }
  validateModeReadsItsFiles(surface, where);
  validateStatusPath(surface, where);
  validateExclude(surface, where);
}

/**
 * `exclude` narrows a glob by naming the files inside it that ship to nobody.
 *
 * The alternative to having it is worse than it looks. A surface can only be as
 * wide as its narrowest legal glob, so covering an application's error messages
 * meant either listing every directory that holds one — a list that goes stale
 * the day somebody adds a directory — or widening the glob until it swallows the
 * test files, where a fixture that deliberately contains a forbidden term is a
 * finding. Both endings are the same: the surface gets dropped from the manifest
 * and the strings stop being scanned.
 *
 * It is narrowing, not suppression, and the difference is enforced two ways: a
 * pattern that matches nothing is an error rather than a silent no-op, and there
 * is no per-file form of it. `allowRegionBudget` is the reviewed, counted,
 * printed way to exempt shipped text; this is not a second one.
 */
function validateExclude(surface, where) {
  if (surface.exclude === undefined) return;
  if (!Array.isArray(surface.exclude) || surface.exclude.length === 0) {
    throw new Error(`${where} (${surface.id}) has an "exclude" that is not a non-empty array of globs`);
  }
  for (const pattern of surface.exclude) {
    if (typeof pattern !== 'string' || pattern.length === 0) {
      throw new Error(`${where} (${surface.id}) has an empty "exclude" pattern`);
    }
  }
  if (surface.path) {
    throw new Error(
      `${where} (${surface.id}) sets "exclude" beside "path"; a single named file is either a surface or it is not`,
    );
  }
}

/**
 * `statusPath` names the field holding the status value in a catalogue. It is
 * meaningful only where the six-state rule reads a catalogue, and a typo in it
 * would select nothing — so it is parsed at load, not at first use.
 */
function validateStatusPath(surface, where) {
  if (surface.statusPath === undefined) return;
  if (surface.mode !== 'status-catalog') {
    throw new Error(
      `${where} (${surface.id}) sets "statusPath" but its mode is "${surface.mode}"; the status path only applies to mode "status-catalog"`,
    );
  }
  try {
    parseStatusPath(surface.statusPath);
  } catch (error) {
    throw new Error(`${where} (${surface.id}) has an invalid "statusPath": ${error.message}`);
  }
}

/**
 * A surface whose mode cannot read its own file extension scans nothing and
 * reports clean — the silent hole this tool exists to prevent. Refuse the
 * pairing at load rather than discovering it when the files eventually land.
 */
function validateModeReadsItsFiles(surface, where) {
  const readable = MODE_EXTENSIONS[surface.mode];
  const target = surface.path ?? surface.glob;
  const extensions = surface.path
    ? [path.extname(surface.path)].filter((extension) => extension.length > 0)
    : globExtensions(surface.glob);
  if (extensions === null || extensions.length === 0) {
    throw new Error(
      `${where} (${surface.id}) targets "${target}", whose extension is unbounded — mode "${surface.mode}" cannot be shown to read it. Narrow the glob to explicit extensions (${readable.join(', ')}).`,
    );
  }
  const unreadable = extensions.filter((extension) => !readable.includes(extension));
  if (unreadable.length > 0) {
    throw new Error(
      `${where} (${surface.id}) pairs mode "${surface.mode}" with "${target}", which matches ${unreadable.join(', ')} — that mode only reads ${readable.join(', ')}, so those files would be scanned to nothing and the surface would report clean. Split the surface, one mode per extension.`,
    );
  }
}

/**
 * Expand a surface into the concrete repository-relative files it covers.
 * A `required` surface that matches nothing is an error — the manifest and the
 * tree have drifted apart. So is an `exclude` pattern that excludes nothing: a
 * narrowing that narrows nothing is a claim about the tree that has stopped
 * being true, and leaving it in place is how a surface silently regains files
 * nobody meant it to have — or keeps a hole nobody remembers opening.
 */
export function resolveSurfaceFiles(root, surface) {
  if (surface.path) {
    const absolute = path.join(root, surface.path);
    if (fs.existsSync(absolute) && fs.statSync(absolute).isFile()) return [surface.path];
    if (surface.required) {
      throw new Error(`surface "${surface.id}" requires ${surface.path}, which does not exist`);
    }
    return [];
  }
  const matched = matchFiles(root, surface.glob);
  const kept = (surface.exclude ?? []).reduce((files, pattern) => {
    const excluder = globToRegExp(pattern);
    const remaining = files.filter((relative) => !excluder.test(relative));
    if (remaining.length === files.length) {
      throw new Error(
        `surface "${surface.id}" excludes "${pattern}", which matches nothing inside ${surface.glob} — `
          + 'drop the pattern or fix it; an exclusion that excludes nothing hides which files this surface really scans',
      );
    }
    return remaining;
  }, matched);
  if (kept.length === 0 && surface.required) {
    throw new Error(`surface "${surface.id}" requires files matching ${surface.glob}, and none exist`);
  }
  return kept;
}

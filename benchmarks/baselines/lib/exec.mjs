// Process invocation for the baseline runners.
//
// Every external command a baseline runs goes through here, so that a failure
// is a named failure with the command in it rather than an empty string that
// later becomes a zero in a result row.

import { spawnSync } from 'node:child_process';

/** A command failed, or could not be found. */
export class CommandError extends Error {
  constructor(command, detail) {
    super(`\`${command}\` ${detail}`);
    this.name = 'CommandError';
    this.command = command;
  }
}

/**
 * Runs a command and returns `{ code, stdout, stderr }` without throwing.
 * Used where a non-zero exit is information rather than an error.
 */
export function attempt(program, args, options = {}) {
  const result = spawnSync(program, args, {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  });
  if (result.error) {
    return { code: null, stdout: '', stderr: String(result.error.message) };
  }
  return {
    code: result.status,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
  };
}

/** Runs a command, returning trimmed stdout, or throwing a named CommandError. */
export function output(program, args, options = {}) {
  const result = attempt(program, args, options);
  const command = [program, ...args].join(' ');
  if (result.code === null) {
    return failed(command, `could not be run: ${result.stderr.trim()}`);
  }
  if (result.code !== 0) {
    return failed(command, `exited ${result.code}: ${result.stderr.trim()}`);
  }
  return result.stdout.trim();
}

/** Runs a command for its effect, throwing a named CommandError on failure. */
export function run(program, args, options = {}) {
  output(program, args, options);
}

/**
 * Runs a `/bin/sh -c` pipeline.
 *
 * Reserved for the two places a baseline genuinely needs a pipe (`git archive |
 * tar -x`); everything else uses argv directly so no value is ever re-parsed by
 * a shell.
 */
export function shell(script, options = {}) {
  return output('/bin/sh', ['-c', script], options);
}

/** Whether `program` resolves on PATH. */
export function onPath(program) {
  return attempt('/usr/bin/env', ['sh', '-c', `command -v ${program}`]).code === 0;
}

function failed(command, detail) {
  throw new CommandError(command, detail);
}

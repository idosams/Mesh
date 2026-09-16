// Where the Mesh background service is listening, decided the same way on both sides.
//
// # Why this is duplicated in Rust, and why that is safe
//
// `crates/mesh-daemon/examples/serve.rs` resolves the endpoint with the same three rules in the
// same order. The two implementations are deliberately independent — this application has no
// `crates/` directory next to it when it ships, so it cannot read the daemon's answer — and the
// duplication is bounded to ONE fact: a path. It cannot drift silently in the way a message format
// can, because the failure is immediate and total: the window says the service is not running, and
// the service prints the endpoint it bound on its first line. `endpoint.test.ts` pins the rules on
// this side.
//
// # The rules
//
//  1. `--endpoint <path>` on the command line, because a person running two services on one
//     machine has to be able to say which.
//  2. `MESH_DAEMON_ENDPOINT`, so one shell variable points a window and a service at one socket.
//  3. `$HOME/.mesh/run/daemon.sock`, so that the two commands in the README need no arguments.
//
// The last resort when there is no home directory is the temporary directory, which is where a
// sandbox with no `HOME` puts things. There is no fourth rule and no built-in fallback to a
// well-known system path: a socket this application did not expect is a socket it should not talk
// to.
//
// This file imports nothing. `src/app/architecture.test.ts` holds every shipped source to relative
// imports and one Node builtin, and joining three path segments is not worth the exemption.

/** The path segments appended to a home directory when nothing else says where to look. */
export const DEFAULT_ENDPOINT_SEGMENTS: readonly string[] = ['.mesh', 'run', 'daemon.sock'];

/** The environment variable that overrides the default for every Mesh process in a shell. */
export const ENDPOINT_VARIABLE = 'MESH_DAEMON_ENDPOINT';

/** The subset of the environment this resolution reads. */
export type Environment = {
  readonly [key: string]: string | undefined;
};

/** Join path segments with a single separator, whatever trailing separators the parts carry. */
const joinPath = (base: string, segments: readonly string[]): string => {
  const trimmed = base.endsWith('/') ? base.replace(/\/+$/, '') : base;
  return [trimmed, ...segments].join('/');
};

/** The value of `--endpoint`, when the arguments carry one. */
export const endpointArgument = (argv: readonly string[]): string | null => {
  const at = argv.indexOf('--endpoint');
  if (at === -1) return null;
  const value = argv[at + 1];
  if (value === undefined || value.length === 0 || value.startsWith('--')) {
    throw new Error('`--endpoint` needs a path to the Mesh background service.');
  }
  return value;
};

/** Where this window will look for the background service. */
export const resolveEndpoint = (argv: readonly string[], env: Environment): string => {
  const argument = endpointArgument(argv);
  if (argument !== null) return argument;
  const configured = env[ENDPOINT_VARIABLE];
  if (configured !== undefined && configured.length > 0) return configured;
  const home = env['HOME'] ?? env['TMPDIR'] ?? '/tmp';
  return joinPath(home, DEFAULT_ENDPOINT_SEGMENTS);
};

/** The value of a `--name value` argument, when the arguments carry one. */
export const namedArgument = (argv: readonly string[], name: string): string | null => {
  const at = argv.indexOf(name);
  if (at === -1) return null;
  const value = argv[at + 1];
  if (value === undefined || value.length === 0 || value.startsWith('--')) {
    throw new Error(`\`${name}\` needs a value.`);
  }
  return value;
};

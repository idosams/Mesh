export const PROVE_RENDERED_APP_USAGE = `Usage: npm run tauri:prove-local --prefix apps/desktop -- [options]

Launch the packaged Mesh app in an isolated disposable environment and verify the
rendered alpha journey.

Options:
  --screenshot <path>  Save a screenshot of the verified window.
  --seed-repository <path>
                       Restore a clean Git HEAD into the disposable source before
                       adding proof fixtures. The original repository stays read-only.
  -h, --help           Show this help without launching Mesh.`;

export function parseProofArguments(argv) {
  let screenshot = null;
  let seedRepository = null;
  let help = false;

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '-h' || argument === '--help') {
      if (help) throw new Error(`${argument} may be provided only once`);
      if (argv.length !== 1) throw new Error(`${argument} must be used without other arguments`);
      help = true;
      continue;
    }
    if (argument === '--screenshot') {
      if (screenshot !== null) throw new Error('--screenshot may be provided only once');
      const value = argv[index + 1];
      if (!value || value.startsWith('-')) {
        throw new Error('--screenshot requires an absolute or relative output path');
      }
      screenshot = value;
      index += 1;
      continue;
    }
    if (argument === '--seed-repository') {
      if (seedRepository !== null) throw new Error('--seed-repository may be provided only once');
      const value = argv[index + 1];
      if (!value || value.startsWith('-')) {
        throw new Error('--seed-repository requires an absolute or relative Git worktree path');
      }
      seedRepository = value;
      index += 1;
      continue;
    }
    throw new Error(`unknown argument: ${argument}`);
  }

  return { help, screenshot, seedRepository };
}

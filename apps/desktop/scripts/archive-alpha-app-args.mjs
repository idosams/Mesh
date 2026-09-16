export const ARCHIVE_ALPHA_APP_USAGE = `Usage: npm run tauri:archive-alpha --prefix apps/desktop -- [--help]

Build, verify, and package one exact clean Git revision of the local macOS alpha.

Options:
  -h, --help  Show this help without building Mesh or writing release artifacts.`;

export function parseArchiveArguments(argv) {
  if (argv.length === 0) return { help: false };
  if (argv.length === 1 && (argv[0] === '-h' || argv[0] === '--help')) {
    return { help: true };
  }
  if (argv.includes('-h') || argv.includes('--help')) {
    throw new Error('help must be used without other arguments');
  }
  throw new Error(`unknown argument: ${argv[0]}`);
}

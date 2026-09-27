export function captureProofArguments(argv) {
  const values = { app: null, revision: null };
  for (let index = 0; index < argv.length; index++) {
    const key = argv[index] === '--app' ? 'app' : argv[index] === '--revision' ? 'revision' : null;
    if (!key || values[key] !== null || !argv[index + 1] || argv[index + 1].startsWith('-')) {
      throw new Error('Use --app <bundle> --revision <exact-commit>, or no arguments for development binaries');
    }
    values[key] = argv[++index];
  }
  if ((values.app === null) !== (values.revision === null)
    || (values.revision !== null && !/^[0-9a-f]{40}$/.test(values.revision))) {
    throw new Error('Packaged capture proof requires both an app and an exact lowercase 40-character revision');
  }
  return values;
}

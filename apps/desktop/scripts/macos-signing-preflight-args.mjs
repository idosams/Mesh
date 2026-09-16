export const MACOS_SIGNING_PREFLIGHT_USAGE = `Usage: npm run tauri:signing-preflight --prefix apps/desktop -- [options]

Verify the external Apple identity required for a protected-approval alpha build.

Options:
  --profile <path>          Developer ID provisioning profile for dev.mesh.desktop
  --notary-profile <name>  notarytool Keychain profile name
  --identity <sha1>        exact Developer ID Application identity when more than one exists
  --json                   print one machine-readable readiness record
  -h, --help               show this message without inspecting Keychain or Apple tools`;

function takeValue(argv, index, option) {
  const value = argv[index + 1];
  if (value === undefined || value === '' || value.startsWith('-')) {
    throw new Error(`${option} requires a value`);
  }
  return value;
}

export function parseMacosSigningPreflightArguments(argv) {
  const options = {
    help: false,
    json: false,
    profile: null,
    notaryProfile: null,
    identity: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '-h' || argument === '--help') {
      if (argv.length !== 1) throw new Error('help must be used without other arguments');
      options.help = true;
    } else if (argument === '--json') {
      if (options.json) throw new Error('--json may be supplied only once');
      options.json = true;
    } else if (argument === '--profile') {
      if (options.profile !== null) throw new Error('--profile may be supplied only once');
      options.profile = takeValue(argv, index, argument);
      index += 1;
    } else if (argument === '--notary-profile') {
      if (options.notaryProfile !== null) {
        throw new Error('--notary-profile may be supplied only once');
      }
      options.notaryProfile = takeValue(argv, index, argument);
      index += 1;
    } else if (argument === '--identity') {
      if (options.identity !== null) throw new Error('--identity may be supplied only once');
      options.identity = takeValue(argv, index, argument).toUpperCase();
      if (!/^[0-9A-F]{40}$/.test(options.identity)) {
        throw new Error('--identity must be one exact 40-character SHA-1 fingerprint');
      }
      index += 1;
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  return options;
}

import { open } from 'node:fs/promises';

export async function writeNewPrivateScreenshot(path, bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
    throw new Error('the verified screenshot output bytes were invalid');
  }
  const output = await open(path, 'wx', 0o600);
  try {
    await output.writeFile(bytes);
    await output.sync();
    await output.chmod(0o600);
    const metadata = await output.stat();
    if (!metadata.isFile()
      || metadata.nlink !== 1
      || metadata.size !== bytes.length
      || (metadata.mode & 0o777) !== 0o600) {
      throw new Error('the verified screenshot output was not one exact private file');
    }
  } finally {
    await output.close();
  }
}

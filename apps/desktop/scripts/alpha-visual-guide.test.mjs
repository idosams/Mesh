import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const desktop = new URL('../', import.meta.url);
const visualGuide = await readFile(new URL('ALPHA-VISUAL-GUIDE.html', desktop), 'utf8');
const screenshotNames = [
  'review-workbench.jpg',
  'review-pdf.jpg',
  'review-text-diff.jpg',
];

test('the friend visual guide is self-contained, truthful, and follows the alpha journey', async () => {
  assert.match(visualGuide, /Make one agent change\. Understand it\. Recover it safely\./);
  assert.match(visualGuide, /Six visible checkpoints/);
  assert.match(visualGuide, /Finish agent handoff/);
  assert.match(visualGuide, /Record reviewed version/);
  assert.match(visualGuide, /Review displays an exact recorded version, not every live filesystem change/);
  assert.match(visualGuide, /Approval is unavailable in the ad-hoc build/);
  assert.match(visualGuide, /fictional data rendered by the exact React review components/);
  assert.match(visualGuide, /interface previews, not a claim that the pictured example was processed by the native app/);
  assert.match(visualGuide, /original project is the backup/);
  assert.match(visualGuide, /Do not select the original project/);
  assert.doesNotMatch(visualGuide, /https?:\/\//);
  assert.doesNotMatch(visualGuide, /<script\b/i);

  const imageSources = [...visualGuide.matchAll(/<img\s+src="([^"]+)"\s+alt="([^"]+)"/g)];
  assert.deepEqual(
    imageSources.map((match) => match[1]),
    screenshotNames.map((name) => `alpha-guide/screenshots/${name}`),
  );
  assert.equal(imageSources.every((match) => match[2].length >= 30), true);

  for (const name of screenshotNames) {
    const screenshot = await readFile(new URL(`alpha-guide/screenshots/${name}`, desktop));
    assert.equal(screenshot.subarray(0, 3).toString('hex'), 'ffd8ff');
    assert.ok(screenshot.length > 100_000, `${name} must contain a real reviewed interface capture`);
  }
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import sharp from 'sharp';
for (const name of ['gallery', 'queue', 'settings']) {
  test(`${name}: full-size WebP preserves visible native pixels`, async () => {
    const original = sharp(
      new URL(`../src/assets/screenshots/${name}.png`, import.meta.url)
        .pathname,
    );
    const meta = await original.metadata();
    assert.equal(meta.width, 1280);
    assert.equal(meta.height, 900);
    const output = sharp(
      new URL(`../public/screenshots/${name}.webp`, import.meta.url).pathname,
    );
    const expected = await original.ensureAlpha().raw().toBuffer();
    const actual = await output.ensureAlpha().raw().toBuffer();
    // WebP may change invisible RGB values under fully transparent window corners.
    // Alpha and every visible pixel must still match exactly.
    for (let i = 0; i < expected.length; i += 4) {
      if (expected[i + 3] === 0 && actual[i + 3] === 0) {
        expected.fill(0, i, i + 3);
        actual.fill(0, i, i + 3);
      }
    }
    assert.ok(actual.equals(expected), 'No visible pixel or alpha difference');
  });
}

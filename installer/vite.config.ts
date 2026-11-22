// Two builds. The page (default) needs NEWT_RELEASE, a release folder holding
// newt.bin, newt.manifest and the firmware's NOTICES.txt; the pair is checked
// as the page will check it, and all three are copied into the site, since
// browsers cannot read GitHub Release assets.
// The gallery (--mode gallery) renders every screen from fixtures, for review.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig, type Plugin } from 'vitest/config';
import { checkNewt } from './src/image.ts';

function newtRelease(folder: string | undefined): Plugin {
  return {
    name: 'newt-release',
    async generateBundle() {
      if (!folder) throw new Error('Set NEWT_RELEASE to a folder holding newt.bin and newt.manifest');
      const manifest = readFileSync(resolve(folder, 'newt.manifest'));
      const image = new Uint8Array(readFileSync(resolve(folder, 'newt.bin')));
      const firmware = await checkNewt(manifest.toString('latin1'), image);
      if (!firmware) throw new Error(`${folder}: newt.bin does not match newt.manifest, or is not an intact Newt image`);
      this.emitFile({ type: 'asset', fileName: 'firmware/newt.manifest', source: manifest });
      this.emitFile({ type: 'asset', fileName: 'firmware/newt.bin', source: image });
      // newt.bin's own notices travel with it (docs/NOTICES.txt at its release).
      this.emitFile({ type: 'asset', fileName: 'firmware/NOTICES.txt', source: readFileSync(resolve(folder, 'NOTICES.txt')) });
      this.emitFile({ type: 'asset', fileName: 'NOTICES.txt', source: readFileSync(resolve(import.meta.dirname, 'NOTICES.txt')) });
      console.log(`Bundled ${firmware.name} from ${folder}`);
    },
  };
}

export default defineConfig(({ mode }) => {
  const gallery = mode === 'gallery';
  return {
    base: './',
    plugins: gallery ? [] : [newtRelease(process.env.NEWT_RELEASE)],
    build: {
      outDir: gallery ? 'dist-gallery' : 'dist',
      emptyOutDir: true,
      // The gallery is opened from disk for screenshots: one classic script.
      rolldownOptions: gallery ? { input: resolve(import.meta.dirname, 'gallery.html'), output: { format: 'iife' } } : {},
      modulePreload: !gallery,
      // From disk the gallery's asset URLs would resolve against the page: inline them.
      assetsInlineLimit: gallery ? 1 << 20 : 4096,
    },
    test: { include: ['test/**/*.test.ts'], environment: 'node' },
  };
});

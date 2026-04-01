const esbuild = require('esbuild');

// IIFE bundle for <script> tag — exposes window.Synx
esbuild.buildSync({
  entryPoints: ['src/browser.ts'],
  bundle: true,
  minify: true,
  sourcemap: true,
  format: 'iife',
  globalName: 'SynxModule',
  platform: 'browser',
  target: ['es2020'],
  outfile: 'dist/synx.browser.js',
  footer: {
    js: 'if(typeof window!=="undefined")window.Synx=SynxModule.default;',
  },
});

// ESM bundle for import
esbuild.buildSync({
  entryPoints: ['src/browser.ts'],
  bundle: true,
  minify: true,
  sourcemap: true,
  format: 'esm',
  platform: 'browser',
  target: ['es2020'],
  outfile: 'dist/synx.browser.mjs',
});

console.log('Browser bundles built:');
console.log('  dist/synx.browser.js  (IIFE — window.Synx)');
console.log('  dist/synx.browser.mjs (ESM — import { Synx })');

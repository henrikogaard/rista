const esbuild = require('esbuild')
const fs = require('fs-extra')
const path = require('path')

const root = path.resolve(__dirname, '..')
const outDir = path.join(root, 'dist', 'app')
const minify = process.argv.includes('--minify')
const watch = process.argv.includes('--watch')

// Fonts to copy from node_modules/@fontsource — weights match the Google Fonts
// request that was removed from index.html (issue #37: self-host fonts).
const FONTSOURCE_FONTS = [
  { pkg: 'dm-sans',         weights: ['300', '300-italic', '400', '400-italic', '500'] },
  { pkg: 'dm-mono',         weights: ['400', '500'] },
  { pkg: 'b612',            weights: ['400', '700'] },
  { pkg: 'figtree',         weights: ['400', '500', '600'] },
  { pkg: 'fira-code',       weights: ['400', '500'] },
  { pkg: 'ibm-plex-mono',   weights: ['400', '500'] },
  { pkg: 'ibm-plex-sans',   weights: ['400', '500', '600'] },
  { pkg: 'inter',           weights: ['400', '500', '600'] },
  { pkg: 'jetbrains-mono',  weights: ['400', '500'] },
  { pkg: 'lora',            weights: ['400', '500', '600'] },
  { pkg: 'merriweather',    weights: ['400', '700'] },
  { pkg: 'nunito',          weights: ['400', '600', '700'] },
  { pkg: 'source-code-pro', weights: ['400', '500'] },
  { pkg: 'source-serif-4',  weights: ['400', '600'] },
  { pkg: 'space-grotesk',   weights: ['400', '500', '700'] },
  { pkg: 'work-sans',       weights: ['400', '500', '600'] },
]

async function copyFonts() {
  for (const { pkg, weights } of FONTSOURCE_FONTS) {
    const srcPkg = path.join(root, 'node_modules', '@fontsource', pkg)
    const dstPkg = path.join(outDir, 'fonts', pkg)
    // Copy only the CSS files for the weights we actually use
    for (const w of weights) {
      const cssFile = `${w}.css`
      await fs.copy(path.join(srcPkg, cssFile), path.join(dstPkg, cssFile))
    }
    // Copy all woff2/woff files (the CSS uses relative ./files/ paths)
    await fs.copy(path.join(srcPkg, 'files'), path.join(dstPkg, 'files'))
  }
}

async function copyStaticAssets() {
  await fs.emptyDir(outDir)
  await fs.copy(path.join(root, 'public'), outDir, {
    filter: (src) => !src.endsWith(path.join('public', 'index.html')),
  })
  await fs.copy(
    path.join(root, 'src', 'renderer', 'styles', 'main.css'),
    path.join(outDir, 'styles', 'main.css')
  )
  await fs.copy(path.join(root, 'public', 'index.html'), path.join(outDir, 'index.html'))
}

async function build() {
  await copyStaticAssets()
  await copyFonts()
  const context = await esbuild.context({
    entryPoints: [path.join(root, 'src', 'renderer', 'index.js')],
    bundle: true,
    outfile: path.join(outDir, 'renderer.js'),
    loader: { '.js': 'jsx' },
    platform: 'browser',
    minify,
    sourcemap: !minify,
  })

  if (watch) {
    await context.watch()
    console.log('Watching renderer assets...')
    return
  }

  await context.rebuild()
  await context.dispose()
}

build().catch((err) => {
  console.error(err)
  process.exit(1)
})

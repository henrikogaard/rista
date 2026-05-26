const esbuild = require('esbuild')
const fs = require('fs-extra')
const path = require('path')

const root = path.resolve(__dirname, '..')
const outDir = path.join(root, 'dist', 'app')
const minify = process.argv.includes('--minify')
const watch = process.argv.includes('--watch')

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

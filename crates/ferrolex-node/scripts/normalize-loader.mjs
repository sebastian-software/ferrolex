import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const manifest = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'))
const loaderPath = join(packageRoot, 'index.js')
let loader = readFileSync(loaderPath, 'utf8')

const marker = 'const loadErrors = []\n'
const dynamicVersion = "const packageVersion = require('./package.json').version\n"
const alreadyNormalized = loader.includes(dynamicVersion)
if (!alreadyNormalized) {
  if (!loader.includes(marker)) {
    throw new Error('napi-rs loader no longer contains the expected insertion point')
  }
  loader = loader.replace(marker, `${marker}${dynamicVersion}`)
}

const comparison = `bindingPackageVersion !== '${manifest.version}'`
const message = `expected ${manifest.version} but got`
const comparisons = loader.split(comparison).length - 1
const messages = loader.split(message).length - 1
if (comparisons !== messages || (!alreadyNormalized && comparisons === 0)) {
  throw new Error(
    `unexpected napi-rs version checks: ${comparisons} comparisons and ${messages} messages`,
  )
}

loader = loader
  .replaceAll(comparison, 'bindingPackageVersion !== packageVersion')
  .replaceAll(message, 'expected ${packageVersion} but got')

if (loader.includes(manifest.version)) {
  throw new Error('the normalized loader still embeds the package version')
}
writeFileSync(loaderPath, loader)

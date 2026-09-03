import {
  copyFileSync,
  existsSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const workspaceRoot = resolve(packageRoot, '../..')
const licenses = ['LICENSE-APACHE', 'LICENSE-MIT']

for (const license of licenses) {
  const source = join(workspaceRoot, license)
  copyFileSync(source, join(packageRoot, license))
}

const npmRoot = join(packageRoot, 'npm')
if (existsSync(npmRoot)) {
  for (const directory of readdirSync(npmRoot, { withFileTypes: true })) {
    if (!directory.isDirectory()) continue
    const platformRoot = join(npmRoot, directory.name)
    for (const license of licenses) {
      copyFileSync(
        join(workspaceRoot, license),
        join(platformRoot, license),
      )
    }
    const manifestPath = join(platformRoot, 'package.json')
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
    manifest.files = [...new Set([...manifest.files, ...licenses])]
    manifest.publishConfig = { ...manifest.publishConfig, provenance: true }
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  }
}

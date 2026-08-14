# ferrolex language server

`ferrolex-lsp` is a generic Language Server Protocol server over stdio. It is
editor-neutral: any LSP client can launch `cargo run -p ferrolex-lsp` during
development, or install a versioned server binary from GitHub Releases.

## Capabilities

- full document synchronization with incremental-change application;
- `textDocument/publishDiagnostics` for unknown words, with UTF-16 LSP ranges;
- `textDocument/codeAction` quick fixes for bounded spelling suggestions;
- whole-identifier replacements with the existing case-preserving recombination;
- an inline `ferrolex:ignore` action using the configured comment prefix; and
- `ferrolex.addToDictionary`, which updates and, when configured, persists the
  user dictionary before diagnostics are refreshed.

`workspace/didChangeConfiguration` reloads the server configuration and
reanalyzes open documents. It does not depend on a particular editor extension.

## Configuration

Set this object either in `initializationOptions.ferrolex` or under the
`ferrolex` key of `workspace/didChangeConfiguration`:

```json
{
  "words": ["ferrolex", "Project"],
  "ignoredWords": ["generated"],
  "commentPrefix": "//",
  "userDictionaryPath": "/absolute/path/to/ferrolex-user-words.txt"
}
```

`words` is the small plain-word-list dictionary used by the generic spike;
production dictionary selection remains a separate integration decision.
`userDictionaryPath` uses ferrolex's deterministic plain-word-list format.

## Install a released server

GitHub Releases provide exactly one archive for each supported platform, plus
`SHA256SUMS`. An archive contains only the `ferrolex-lsp` server binary (or
`ferrolex-lsp.exe` on Windows), `LICENSE-APACHE`, `LICENSE-MIT`, and
`NOTICE.txt`. It contains no dictionary data and performs no dictionary
download. Dictionary selection remains caller-controlled under
[ADR-0007](adr/0007-dictionary-distribution.md).

| Platform | Release asset |
| --- | --- |
| macOS x86_64 | `ferrolex-lsp-<VERSION>-x86_64-apple-darwin.tar.gz` |
| macOS arm64 | `ferrolex-lsp-<VERSION>-aarch64-apple-darwin.tar.gz` |
| Ubuntu x86_64 | `ferrolex-lsp-<VERSION>-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `ferrolex-lsp-<VERSION>-x86_64-pc-windows-msvc.zip` |

Download the matching asset and `SHA256SUMS` from the same release tag. Before
extracting it, verify only that asset's manifest entry. On macOS, run:

```sh
asset="ferrolex-lsp-<VERSION>-<TARGET>.tar.gz"
entry=$(grep -F -- "  $asset" SHA256SUMS) || exit 1
printf '%s\n' "$entry" | shasum -a 256 -c -
```

On Ubuntu, run the same selection with `sha256sum`:

```sh
asset="ferrolex-lsp-<VERSION>-<TARGET>.tar.gz"
entry=$(grep -F -- "  $asset" SHA256SUMS) || exit 1
printf '%s\n' "$entry" | sha256sum -c -
```

On Windows PowerShell, verify the downloaded Windows asset with:

```powershell
$asset = "ferrolex-lsp-<VERSION>-x86_64-pc-windows-msvc.zip"
$expected = ((Get-Content SHA256SUMS | Where-Object { $_.EndsWith("  $asset") }) -split "  ")[0]
if ((Get-FileHash $asset -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw "SHA-256 mismatch" }
```

After a successful verification, install the binary in a directory on your
`PATH`. For macOS or Ubuntu, this user-local installation avoids administrator
permissions:

```sh
VERSION=<VERSION>
TARGET=<x86_64-apple-darwin|aarch64-apple-darwin|x86_64-unknown-linux-gnu>
tar -xzf "ferrolex-lsp-$VERSION-$TARGET.tar.gz"
mkdir -p "$HOME/.local/bin"
install -m 755 "ferrolex-lsp-$VERSION-$TARGET/ferrolex-lsp" "$HOME/.local/bin/ferrolex-lsp"
```

For Windows PowerShell, use a versioned installation directory and configure
your LSP client with the resulting absolute path:

```powershell
$version = "<VERSION>"
$target = "x86_64-pc-windows-msvc"
$root = Join-Path $env:LOCALAPPDATA "ferrolex-lsp"
Expand-Archive "ferrolex-lsp-$version-$target.zip" -DestinationPath $root -Force
"$root\ferrolex-lsp-$version-$target\ferrolex-lsp.exe"
```

To update, repeat the verification and installation steps with the newer
`<VERSION>`; the macOS and Ubuntu command replaces the user-local binary, while
Windows installs the new version beside the old one. To uninstall, delete
`$HOME/.local/bin/ferrolex-lsp` on macOS or Ubuntu, or remove the
`%LOCALAPPDATA%\ferrolex-lsp` directory on Windows.

The declared support matrix is only the four platforms in the table. Other
targets can build from source but are not release-supported.

## Compatibility and distribution policy

`ferrolex-lsp` is **maintained experimental**. Its documented stdio methods,
configuration, diagnostics, and code actions are maintained, but protocol and
configuration changes may occur in a minor release with release notes. The
server makes no stable ABI, package, protocol, Marketplace, or editor-client
compatibility promise. The full tier and promotion criteria are in
[ADR-0010](adr/0010-external-integration-support-tiers.md).

This release channel covers only the standalone LSP server binary. VS Code
Marketplace publication, C ABI distribution, npm publication, and PyPI
publication are out of scope.

## Scope decision

This is a **maintained experimental generic LSP implementation**. The protocol
server, diagnostics, quick fixes, user-dictionary flow, configuration reload,
and incremental document handling ship in the workspace. It is the first
selected external distribution surface. Editor packaging remains intentionally
separate. The compatibility, dictionary, and promotion boundaries are
authoritative in [ADR-0010](adr/0010-external-integration-support-tiers.md).

# ferrolex-cli

Reference and diagnostic command-line interface for [ferrolex](https://github.com/sebastian-software/ferrolex).

The binary checks plain text and source files, imports or validates Hunspell
dictionaries, manages reviewed dictionary caches, and exposes machine-readable
JSON output for automation.

```sh
cargo run --bin ferrolex -- check --dictionary words.txt README.md
cargo run --bin ferrolex -- dictionary list
```

The CLI is a supporting interface. Library consumers should use the public
Rust API from the [`ferrolex`](https://docs.rs/ferrolex) crate directly.

<!-- ferramenta-family:registry:start -->
Part of the [Ferramenta](https://ferramenta.dev) family of Rust-native developer tools by [Sebastian Software](https://oss.sebastian-software.com).
Siblings: [ferroni](https://github.com/sebastian-software/ferroni), [ferriki](https://github.com/sebastian-software/ferriki), [ferromark](https://github.com/sebastian-software/ferromark), [ferralk](https://github.com/sebastian-software/ferralk), [ferrovia](https://github.com/sebastian-software/ferrovia), [ferrocat](https://github.com/sebastian-software/ferrocat), and [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:registry:end -->

<!-- sebastian-software-branding:start -->
<p align="center">
  <a href="https://oss.sebastian-software.com">
    <img src="https://raw.githubusercontent.com/sebastian-software/ferramenta/main/app/assets/logos/sebastian-software.svg" alt="Sebastian Software" width="240" />
  </a>
</p>

<p align="center">
  <a href="https://oss.sebastian-software.com">Open Source at Sebastian Software</a><br />
  Copyright &copy; 2026 Sebastian Software GmbH
</p>
<!-- sebastian-software-branding:end -->

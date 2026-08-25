# Command-line workflow

Use `ferrolex --help` for the command synopsis and `ferrolex --version` (or
`-V`) to report the installed version. Long options that accept a value support
both `--option value` and `--option=value` forms.

## Exit status

- `0`: the command completed successfully; `check` found no misspelling.
- `1`: `check`, `analyze`, or a strict validation operation reported a
  spelling or dictionary finding.
- `2`: the command invocation is invalid. The diagnostic is followed by the
  usage synopsis on standard error.
- `3`: an operational failure occurred, such as a missing input file, failed
  cache access, or failed compilation. The diagnostic is written to standard
  error without a usage dump.

Normal findings are written to standard output so callers can process them;
usage and operational diagnostics are written to standard error.

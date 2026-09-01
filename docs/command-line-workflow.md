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

## Output formats

`check`, `suggest`, `analyze`, and `validate` accept
`--format text|json`. The default `text` format is stable and intended for
people, grep-style consumers, and editor problem matchers:

- word checks use `accepted: WORD` or `misspelled: WORD`;
- file checks and spelling analysis use
  `PATH:LINE:COLUMN: misspelled: WORD`;
- analysis suggestions use
  `PATH:LINE:COLUMN: suggestion: WORD (distance N)`;
- malformed directives use
  `PATH:LINE:COLUMN: malformed directive: PROBLEM`;
- standalone suggestions use `suggestion: WORD (distance N)`; and
- validation emits importer diagnostics as
  `SOURCE:LINE: SEVERITY[DIRECTIVE]: MESSAGE` and reports valid inputs as
  `valid: PATH`.

`--format json` writes one compact JSON object per line to standard output.
Every line is independently parseable; fields not listed for a record type are
not implied. Paths use the same display representation as text output.

| `type` | Emitted by | Stable fields |
| --- | --- | --- |
| `word` | `check WORD` | `command`, `word`, `status` (`accepted` or `misspelled`) |
| `finding` | `check --file`, `analyze` | `command`, `kind`, `path`, `line`, `column`; spelling findings add `word`, and analyze spelling findings add a `suggestions` array |
| `suggestion` | `suggest` | `word`, `distance` |
| `suggestion-summary` | `suggest` | `word`, `completeness`, `complete`, `hint` |
| `diagnostic` | `validate` | `command`, `source`, `line`, `directive`, `severity`, `message` |
| `validation` | `validate` | `path`, `status` (`valid` or `invalid`) |

Suggestion completeness codes are `complete`, `candidate-limit`,
`edit-budget`, `query-too-long`, and `related-seed-too-long`. Directive problem
codes are `missing-ignored-words`, `unexpected-arguments`, `unknown-directive`,
and `unsupported` for a future problem unknown to this CLI version.

JSON mode does not change exit statuses. Usage and operational errors remain
human-readable on standard error, so stdout is never mixed with a partial
error envelope. Non-fatal operational warnings, such as skipping a non-UTF-8
analysis input, also remain on standard error.

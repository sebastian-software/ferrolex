//! Prints the homepage samples as JSON: real output of the released ferrolex
//! crates at the Git tag pinned in Cargo.toml.
//!
//! `homepage/scripts/render-home-samples.mjs` runs this program and writes its
//! output to `homepage/app/data/home-samples.json`. The program installs the
//! reviewed `LibreOffice` dictionaries into the cache directory given as its
//! only argument, through the release's own digest-checked installer, then
//! imports, checks, explains and suggests with them. No dictionary content is
//! committed; the JSON holds only the words below and what the engine answered.

use std::{env, fs, path::PathBuf, process};

use ferrolex_core::Dictionary as _;
use ferrolex_dictionaries::{DictionaryInstaller, LIBREOFFICE_CATALOG, UreqFetcher, find_locale};
use ferrolex_hunspell::{
    AcceptanceKind, AppliedAffixKind, CasingPath, CompoundComponentRole, HunspellDictionary,
    ImportMode, LookupExplanation, RejectionReason, Severity, import_bytes,
};
use ferrolex_suggest::{Completeness, SuggestConfig, Suggester};
use serde_json::{Value, json};

type Failure = Box<dyn std::error::Error>;

/// One locale's inputs: words to explain, and a list of words to check, the
/// way an application hands over the words it extracted from its text.
struct Locale {
    id: &'static str,
    explain: &'static [&'static str],
    check: &'static [&'static str],
}

const LOCALES: [Locale; 2] = [
    Locale {
        id: "en_US",
        explain: &[
            "walked",
            "WALKED",
            "rewriting",
            "dictionaries",
            "unhappiness",
            "Paris",
            "paris",
        ],
        check: &[
            "The", "editor", "recieved", "a", "mesage", "about", "speling", "in", "Paris",
        ],
    },
    Locale {
        id: "de_DE",
        explain: &["Häuser", "Gelaufen", "Straße", "Wörterbuch"],
        check: &[
            "Die", "Zeitng", "liegt", "vor", "den", "Häusern", "der", "Strase",
        ],
    },
];

/// The budget sample: one query under three work budgets, from a deliberately
/// small one through the default to a larger one.
const BUDGET_LOCALE: &str = "en_US";
const BUDGET_WORD: &str = "recieved";
const SMALL_CANDIDATES: usize = 2_000;
const LARGE_EDIT_CELLS: usize = 20_000_000;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Failure> {
    let cache = PathBuf::from(
        env::args()
            .nth(1)
            .ok_or("usage: ferrolex-homepage-samples <dictionary-cache>")?,
    );
    let installer = DictionaryInstaller::new(UreqFetcher);
    let mut dictionaries = Vec::new();
    let mut explained = Vec::new();
    let mut checked = Vec::new();
    let mut budget = Value::Null;
    for locale in &LOCALES {
        let entry = find_locale(locale.id).ok_or("the catalog lost a homepage locale")?;
        let manifest = entry.manifest()?;
        let files = installer.install(&manifest, &cache)?;
        let aff = fs::read(files.aff_path())?;
        let dic = fs::read(files.dic_path())?;
        let imported = import_bytes(
            manifest.aff().name(),
            &aff,
            manifest.dic().name(),
            &dic,
            ImportMode::Strict,
        )
        .map_err(|error| format!("{} does not import strictly: {error:?}", locale.id))?;
        let dictionary = imported.dictionary();
        let warnings = imported
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.severity() == Severity::Warning)
            .map(|diagnostic| diagnostic.directive().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        dictionaries.push(json!({
            "locale": locale.id,
            "revision": manifest.revision(),
            "license": manifest.license_spdx_expression(),
            "licenseNotice": manifest.license_notice_url(),
            "aff": { "name": manifest.aff().name(), "sha256": manifest.aff().sha256_hex() },
            "dic": { "name": manifest.dic().name(), "sha256": manifest.dic().sha256_hex() },
            "stems": dictionary.stems().count(),
            "warnings": warnings,
        }));
        explained.push(json!({
            "locale": locale.id,
            "words": locale.explain.iter().map(|word| explain(dictionary, word)).collect::<Vec<_>>(),
        }));
        let words = check(dictionary, locale.check, SuggestConfig::default());
        // Determinism: a second, independent run must answer word for word the same.
        assert_eq!(
            words,
            check(dictionary, locale.check, SuggestConfig::default()),
            "suggestions changed between two runs"
        );
        checked.push(json!({ "locale": locale.id, "words": words }));
        if locale.id == BUDGET_LOCALE {
            let budgets = [
                (
                    "small",
                    SuggestConfig {
                        max_candidates: SMALL_CANDIDATES,
                        ..SuggestConfig::default()
                    },
                ),
                ("default", SuggestConfig::default()),
                (
                    "large",
                    SuggestConfig {
                        max_edit_cells: LARGE_EDIT_CELLS,
                        ..SuggestConfig::default()
                    },
                ),
            ];
            budget = json!({
                "locale": locale.id,
                "word": BUDGET_WORD,
                "runs": budgets.iter().map(|(id, config)| {
                    let mut run = suggest(dictionary, BUDGET_WORD, *config);
                    run["id"] = json!(id);
                    run["limits"] = limits(*config);
                    run
                }).collect::<Vec<_>>(),
            });
        }
    }
    let samples = json!({
        "catalog": {
            "locales": LIBREOFFICE_CATALOG.len(),
            "revision": LIBREOFFICE_CATALOG[0].revision(),
        },
        "dictionaries": dictionaries,
        "explain": explained,
        "check": checked,
        "budget": budget,
    });
    println!("{}", serde_json::to_string(&samples)?);
    Ok(())
}

fn limits(config: SuggestConfig) -> Value {
    json!({
        "maxResults": config.max_results,
        "maxEditDistance": config.max_edit_distance,
        "maxCandidates": config.max_candidates,
        "maxEditCells": config.max_edit_cells,
    })
}

fn check(dictionary: &HunspellDictionary, words: &[&str], config: SuggestConfig) -> Vec<Value> {
    words
        .iter()
        .map(|word| {
            if dictionary.contains(word) {
                json!({ "word": word, "correct": true })
            } else {
                let mut result = suggest(dictionary, word, config);
                result["word"] = json!(word);
                result["correct"] = json!(false);
                result
            }
        })
        .collect()
}

/// Suggestions wired the way the released CLI wires them: the dictionary's
/// REP rules and its KEY and MAP ranking signals, with OCONV applied to output.
fn suggest(dictionary: &HunspellDictionary, word: &str, config: SuggestConfig) -> Value {
    let result = Suggester::new(dictionary, config)
        .with_replacement_rules(dictionary.replacement_rules())
        .with_ranking_signals(dictionary.ranking_signals())
        .suggest(word);
    json!({
        "suggestions": result
            .suggestions()
            .iter()
            .map(|suggestion| json!({
                "word": dictionary.normalize_output(suggestion.word()),
                "distance": suggestion.distance(),
            }))
            .collect::<Vec<_>>(),
        "completeness": completeness(result.completeness()),
    })
}

const fn completeness(value: Completeness) -> &'static str {
    match value {
        Completeness::Complete => "complete",
        Completeness::CandidateLimitReached => "candidate-limit",
        Completeness::EditBudgetReached => "edit-budget",
        Completeness::QueryTooLong => "query-too-long",
        Completeness::RelatedSeedTooLong => "related-seed-too-long",
    }
}

fn explain(dictionary: &HunspellDictionary, word: &str) -> Value {
    let explanation: LookupExplanation = dictionary.explain(word);
    if let Some(acceptance) = explanation.accepted() {
        {
            let casing = match acceptance.casing() {
                CasingPath::Exact => json!({ "path": "exact" }),
                CasingPath::CaseFallback { candidate } => {
                    json!({ "path": "fallback", "candidate": candidate })
                }
                _ => json!({ "path": "other" }),
            };
            let kind = match acceptance.kind() {
                AcceptanceKind::Stem { stem } => json!({ "type": "stem", "stem": stem }),
                AcceptanceKind::Affixed { stem, rules } => json!({
                    "type": "affixed",
                    "stem": stem,
                    "rules": rules.iter().map(|rule| json!({
                        "kind": match rule.kind() {
                            AppliedAffixKind::Prefix => "prefix",
                            AppliedAffixKind::Suffix => "suffix",
                        },
                        "strip": rule.strip(),
                        "add": rule.add(),
                    })).collect::<Vec<_>>(),
                }),
                AcceptanceKind::Compound { components } => json!({
                    "type": "compound",
                    "components": components.iter().map(|component| json!({
                        "spelling": component.spelling(),
                        "stem": component.stem(),
                        "role": match component.role() {
                            CompoundComponentRole::Generic => "generic",
                            CompoundComponentRole::Begin => "begin",
                            CompoundComponentRole::Middle => "middle",
                            CompoundComponentRole::End => "end",
                        },
                    })).collect::<Vec<_>>(),
                }),
                AcceptanceKind::Compatibility { detail } => {
                    json!({ "type": "compatibility", "detail": detail })
                }
                _ => json!({ "type": "other" }),
            };
            return json!({ "word": word, "accepted": true, "casing": casing, "kind": kind });
        }
    }
    match explanation.rejected() {
        Some(rejection) => {
            let reason = match rejection.reason() {
                RejectionReason::ForbiddenStem { stem } => {
                    json!({ "type": "forbidden", "stem": stem })
                }
                RejectionReason::NeedsAffix { stem } => {
                    json!({ "type": "needs-affix", "stem": stem })
                }
                RejectionReason::OnlyInCompound { stem } => {
                    json!({ "type": "only-in-compound", "stem": stem })
                }
                RejectionReason::KeepCase { stem } => json!({ "type": "keep-case", "stem": stem }),
                RejectionReason::NoDerivation => json!({ "type": "no-derivation" }),
                _ => json!({ "type": "other" }),
            };
            json!({ "word": word, "accepted": false, "reason": reason })
        }
        None => json!({ "word": word, "accepted": false, "reason": { "type": "other" } }),
    }
}

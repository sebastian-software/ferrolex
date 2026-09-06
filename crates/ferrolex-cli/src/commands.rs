//! Command execution and dictionary integration.

use super::*;

pub(crate) fn explain(command: &ExplainCommand) -> Result<RunOutcome, CliError> {
    let dictionary = load_installed_hunspell_dictionary(&command.hunspell_affix_path)?;
    print!("{}", render_explanation(&dictionary.explain(&command.word)));
    Ok(RunOutcome::Success)
}

pub(crate) fn render_explanation(explanation: &LookupExplanation) -> String {
    let mut output = String::new();
    match explanation {
        LookupExplanation::Accepted(accepted) => render_accepted(&mut output, accepted),
        LookupExplanation::Rejected(rejected) => render_rejected(&mut output, rejected),
        _ => {
            writeln!(output, "status: unsupported diagnostic variant")
                .expect("writing to a String cannot fail");
        }
    }
    output
}

pub(crate) fn render_accepted(output: &mut String, accepted: &Acceptance) {
    writeln!(output, "status: accepted").expect("writing to a String cannot fail");
    match accepted.casing() {
        CasingPath::Exact => writeln!(output, "casing: exact"),
        CasingPath::CaseFallback { candidate } => {
            writeln!(output, "casing: fallback ({candidate})")
        }
        _ => writeln!(output, "casing: compatibility path"),
    }
    .expect("writing to a String cannot fail");
    match accepted.kind() {
        AcceptanceKind::Stem { stem } => {
            writeln!(output, "match: stem\nstem: {stem}").expect("writing to a String cannot fail");
        }
        AcceptanceKind::Affixed { stem, rules } => {
            writeln!(output, "match: affixed\nstem: {stem}")
                .expect("writing to a String cannot fail");
            for (index, rule) in rules.iter().enumerate() {
                let kind = match rule.kind() {
                    AppliedAffixKind::Prefix => "prefix",
                    AppliedAffixKind::Suffix => "suffix",
                };
                writeln!(
                    output,
                    "rule {}: {kind} strip={:?} add={:?}",
                    index + 1,
                    rule.strip(),
                    rule.add()
                )
                .expect("writing to a String cannot fail");
                if !rule.continuation_flags().is_empty() {
                    writeln!(
                        output,
                        "  continuation-flags: {}",
                        rule.continuation_flags().join(", ")
                    )
                    .expect("writing to a String cannot fail");
                }
            }
        }
        AcceptanceKind::Compound { components } => {
            writeln!(output, "match: compound").expect("writing to a String cannot fail");
            for (index, component) in components.iter().enumerate() {
                writeln!(
                    output,
                    "component {}: {} (stem: {}; role: {})",
                    index + 1,
                    component.spelling(),
                    component.stem(),
                    compound_role_label(component.role())
                )
                .expect("writing to a String cannot fail");
            }
        }
        AcceptanceKind::Compatibility { detail } => {
            writeln!(output, "match: compatibility\ndetail: {detail}")
                .expect("writing to a String cannot fail");
        }
        _ => {
            writeln!(output, "match: unsupported diagnostic variant")
                .expect("writing to a String cannot fail");
        }
    }
}

pub(crate) fn render_rejected(output: &mut String, rejected: &Rejection) {
    writeln!(output, "status: rejected").expect("writing to a String cannot fail");
    let reason = match rejected.reason() {
        RejectionReason::ForbiddenStem { stem } => format!("forbidden stem ({stem})"),
        RejectionReason::NeedsAffix { stem } => format!("stem requires an affix ({stem})"),
        RejectionReason::OnlyInCompound { stem } => {
            format!("stem is valid only in a compound ({stem})")
        }
        RejectionReason::KeepCase { stem } => format!("stem requires its stored case ({stem})"),
        RejectionReason::NoDerivation => "no accepted stem or derivation".to_owned(),
        _ => "unsupported diagnostic variant".to_owned(),
    };
    writeln!(output, "reason: {reason}").expect("writing to a String cannot fail");
}

pub(crate) const fn compound_role_label(role: CompoundComponentRole) -> &'static str {
    match role {
        CompoundComponentRole::Generic => "generic",
        CompoundComponentRole::Begin => "begin",
        CompoundComponentRole::Middle => "middle",
        CompoundComponentRole::End => "end",
    }
}

pub(crate) fn suggest(command: &SuggestCommand) -> Result<RunOutcome, CliError> {
    let source = load_analysis_dictionary(
        &command.dictionary_paths,
        &command.compiled_paths,
        &command.hunspell_affix_paths,
    )?;
    if source.is_empty() {
        return Err(CliError::Usage(
            "suggest requires a dictionary option or a workspace/global user dictionary".to_owned(),
        ));
    }
    let replacements = source.replacement_rules();
    let ranking_dictionary = source.hunspell_ranking_dictionary();
    let mut config = SuggestConfig::default();
    if let Some(max_results) = command.max_results {
        config.max_results = max_results;
    }
    if let Some(max_edit_distance) = command.max_edit_distance {
        config.max_edit_distance = max_edit_distance;
    }
    if let Some(max_candidates) = command.max_candidates {
        config.max_candidates = max_candidates;
    }
    if let Some(max_edit_cells) = command.max_edit_cells {
        config.max_edit_cells = max_edit_cells;
    }
    let result = if let Some(dictionary) = ranking_dictionary {
        Suggester::new(&source, config)
            .with_replacement_rules(&replacements)
            .with_ranking_signals(dictionary.ranking_signals())
            .suggest(&command.word)
    } else {
        Suggester::new(&source, config)
            .with_replacement_rules(&replacements)
            .suggest(&command.word)
    };
    for suggestion in result.suggestions() {
        let word = source.normalize_suggestion_output(suggestion.word());
        match command.output_format {
            OutputFormat::Text => {
                println!("suggestion: {word} (distance {})", suggestion.distance());
            }
            OutputFormat::Json => print_json(json!({
                "type": "suggestion",
                "word": word,
                "distance": suggestion.distance(),
            })),
        }
    }
    if command.output_format == OutputFormat::Json {
        print_json(json!({
            "type": "suggestion-summary",
            "word": command.word,
            "completeness": completeness_code(result.completeness()),
            "complete": result.completeness() == Completeness::Complete,
            "hint": incomplete_suggestion_hint(result.completeness(), config),
        }));
    } else if result.completeness() != Completeness::Complete {
        eprintln!(
            "suggestion search incomplete: {}",
            completeness_label(result.completeness())
        );
        if result.suggestions().is_empty() {
            if let Some(hint) = incomplete_suggestion_hint(result.completeness(), config) {
                eprintln!("hint: {hint}");
            }
        }
    }
    Ok(RunOutcome::Success)
}

pub(crate) const fn completeness_code(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::CandidateLimitReached => "candidate-limit",
        Completeness::EditBudgetReached => "edit-budget",
        Completeness::QueryTooLong => "query-too-long",
        Completeness::RelatedSeedTooLong => "related-seed-too-long",
    }
}

pub(crate) fn incomplete_suggestion_hint(
    completeness: Completeness,
    config: SuggestConfig,
) -> Option<String> {
    match completeness {
        Completeness::CandidateLimitReached | Completeness::EditBudgetReached => Some(format!(
            "retry with larger work budgets, for example `--max-candidates {} --max-edit-cells {}`",
            config.max_candidates.saturating_mul(2),
            config.max_edit_cells.saturating_mul(2),
        )),
        Completeness::Complete | Completeness::QueryTooLong | Completeness::RelatedSeedTooLong => {
            None
        }
    }
}

pub(crate) const fn completeness_label(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::CandidateLimitReached => "candidate limit reached",
        Completeness::EditBudgetReached => "edit-distance budget reached",
        Completeness::QueryTooLong => "query exceeds the scalar limit",
        Completeness::RelatedSeedTooLong => "related seed exceeds the scalar limit",
    }
}

pub(crate) fn dictionary(command: &DictionaryCommand) -> Result<RunOutcome, CliError> {
    match command {
        DictionaryCommand::List => {
            for source in LIBREOFFICE_CATALOG {
                println!(
                    "{}\trevision={}\tencoding={}\tspdx={}\tnotice={}",
                    source.locale(),
                    source.revision(),
                    source.encoding().label(),
                    source.license_spdx_expression(),
                    source.license_notice_url()
                );
            }
            Ok(RunOutcome::Success)
        }
        DictionaryCommand::Fetch { locale, cache_path } => {
            let (source, installed) = fetch_catalog_dictionary(locale, cache_path)?;
            println!("fetched: {}", installed.aff_path().display());
            println!("fetched: {}", installed.dic_path().display());
            println!("license: {}", source.license_spdx_expression());
            println!("notice: {}", source.license_notice_url());
            println!(
                "hint: build the runtime cache with `ferrolex dictionary install {locale} --cache {}` before using this catalog dictionary with `--hunspell`",
                cache_path.display()
            );
            Ok(RunOutcome::Success)
        }
        DictionaryCommand::Install { locale, cache_path } => {
            let (source, installed) = fetch_catalog_dictionary(locale, cache_path)?;
            println!("installed: {}", installed.aff_path().display());
            println!("installed: {}", installed.dic_path().display());
            println!("license: {}", source.license_spdx_expression());
            println!("notice: {}", source.license_notice_url());
            install_hunspell_runtime_cache(
                source.locale(),
                installed.aff_path(),
                installed.dic_path(),
                catalog_import_encodings(source.encoding()),
            )
        }
        DictionaryCommand::AddWord { word, path } => add_user_dictionary_word(word, path),
    }
}

pub(crate) fn add_user_dictionary_word(word: &str, path: &Path) -> Result<RunOutcome, CliError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CliError::WriteUserDictionary {
        path: path.to_path_buf(),
        source,
    })?;
    let _lock = UserDictionaryLock::acquire(path)?;
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(CliError::ReadDictionary {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let dictionary = UserDictionary::from_text(Normalization::Nfc, &text);
    let added = dictionary.insert(word).map_err(CliError::InvalidUserWord)?;
    atomic_write(path, &dictionary.to_text())?;
    println!(
        "{}: {}",
        if added { "added" } else { "already present" },
        path.display()
    );
    Ok(RunOutcome::Success)
}

pub(crate) fn atomic_write(path: &Path, text: &str) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CliError::WriteUserDictionary {
        path: path.to_path_buf(),
        source,
    })?;
    sweep_stale_temporary_siblings(path);
    let temporary = temporary_sibling(path);
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| CliError::WriteUserDictionary {
                path: temporary.clone(),
                source,
            })?;
        created = true;
        file.write_all(text.as_bytes())
            .map_err(|source| CliError::WriteUserDictionary {
                path: temporary.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| CliError::WriteUserDictionary {
                path: temporary.clone(),
                source,
            })?;
        fs::rename(&temporary, path).map_err(|source| CliError::WriteUserDictionary {
            path: path.to_path_buf(),
            source,
        })?;
        sync_parent_directory(parent).map_err(|source| CliError::WriteUserDictionary {
            path: parent.to_path_buf(),
            source,
        })
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) struct UserDictionaryLock {
    pub(crate) file: fs::File,
}

impl UserDictionaryLock {
    pub(crate) fn acquire(dictionary_path: &Path) -> Result<Self, CliError> {
        let lock_path = hidden_sibling(dictionary_path, "lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| CliError::WriteUserDictionary {
                path: lock_path.clone(),
                source,
            })?;
        file.lock_exclusive()
            .map_err(|source| CliError::WriteUserDictionary {
                path: lock_path,
                source,
            })?;
        Ok(Self { file })
    }
}

impl Drop for UserDictionaryLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub(crate) fn hidden_sibling(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or(std::ffi::OsStr::new("words")));
    name.push(".");
    name.push(suffix);
    parent.join(name)
}

pub(crate) fn temporary_sibling(path: &Path) -> PathBuf {
    hidden_sibling(
        path,
        &format!(
            "tmp-{}-{}",
            std::process::id(),
            CACHE_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ),
    )
}

pub(crate) fn sweep_stale_temporary_siblings(path: &Path) {
    let parent = path.parent().unwrap_or(Path::new("."));
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let current_prefix = format!(".{name}.tmp-");
    let legacy_runtime_prefix = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| format!("{stem}.tmp-"));
    let legacy_user_prefix = format!(".{name}.");
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let legacy_user_pid = file_name
            .strip_prefix(&legacy_user_prefix)
            .and_then(|value| value.strip_suffix(".tmp"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()));
        let belongs_to_path = file_name.starts_with(&current_prefix)
            || legacy_runtime_prefix
                .as_deref()
                .is_some_and(|prefix| file_name.starts_with(prefix))
            || legacy_user_pid;
        if belongs_to_path && file_is_stale(&entry.path(), STALE_TEMPORARY_FILE_AGE) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub(crate) fn file_is_stale(path: &Path, maximum_age: Duration) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= maximum_age)
}

#[cfg(unix)]
pub(crate) fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "all platforms share one fallible directory-sync call site"
)]
pub(crate) fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn fetch_catalog_dictionary(
    locale: &str,
    cache_path: &Path,
) -> Result<(LibreOfficeDictionary, InstalledDictionary), CliError> {
    let source = find_locale(locale).ok_or_else(|| {
        CliError::Usage(format!(
            "unsupported LibreOffice locale `{locale}`; run `ferrolex dictionary list`"
        ))
    })?;
    let manifest = source.manifest().map_err(CliError::DictionaryManifest)?;
    let aff_url = source.aff_url();
    let host = aff_url
        .strip_prefix("https://")
        .and_then(|url| url.split('/').next())
        .unwrap_or("the pinned dictionary source");
    eprintln!("fetching {locale} from {host}...");
    let installed = DictionaryInstaller::new(UreqFetcher)
        .install(&manifest, cache_path)
        .map_err(CliError::FetchDictionary)?;
    Ok((source, installed))
}

pub(crate) fn compile(command: &CompileCommand) -> Result<RunOutcome, CliError> {
    let (compiled, description) = match &command.input {
        CompileInput::WordList(path) => {
            eprintln!("reading word list from {}...", path.display());
            let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
                path: path.clone(),
                source,
            })?;
            if is_frequency_word_list(&text) {
                eprintln!("building frequency-annotated word-list artifact...");
                (
                    compile_frequency_word_list(&text).map_err(CliError::CompileFrequencyList)?,
                    "frequency-annotated words".to_owned(),
                )
            } else {
                let dictionary = WordList::from_text(Normalization::Exact, &text);
                eprintln!("building word-list artifact...");
                (
                    compile_words(dictionary.words()).map_err(CliError::CompileDictionary)?,
                    format!("{} words", dictionary.len()),
                )
            }
        }
        CompileInput::Hunspell { aff_path, dic_path } => {
            eprintln!(
                "importing Hunspell sources {} and {}...",
                aff_path.display(),
                dic_path.display()
            );
            let (import, sources) = import_hunspell_files(aff_path, dic_path, None, true)?;
            let dictionary = match import {
                Ok(dictionary) => dictionary,
                Err(error) => {
                    for diagnostic in error.diagnostics() {
                        print_import_diagnostic_to_stderr(diagnostic);
                    }
                    eprintln!("error: could not compile Hunspell dictionary");
                    return Ok(RunOutcome::Failure);
                }
            };
            for diagnostic in dictionary.diagnostics() {
                print_import_diagnostic(diagnostic);
            }
            let lexemes = dictionary.ir().lexemes.len();
            eprintln!("building Hunspell runtime artifact...");
            (
                compile_runtime_artifact(dictionary.dictionary(), sources)
                    .map_err(CliError::CompileHunspellCache)?,
                format!("Hunspell, {lexemes} lexemes"),
            )
        }
    };
    eprintln!(
        "writing compiled artifact to {}...",
        command.output_path.display()
    );
    fs::write(&command.output_path, compiled).map_err(|source| CliError::WriteArtifact {
        path: command.output_path.clone(),
        source,
    })?;

    println!(
        "compiled: {} ({description})",
        command.output_path.display(),
    );
    Ok(RunOutcome::Success)
}

pub(crate) fn check(command: &CheckCommand) -> Result<RunOutcome, CliError> {
    let checker = load_checker(
        &command.dictionary_paths,
        &command.compiled_paths,
        &command.hunspell_affix_paths,
    )?;

    match &command.target {
        CheckTarget::Word(word) => Ok(check_word(&checker, word, command.output_format)),
        CheckTarget::Inputs(inputs) => check_inputs(&checker, inputs, command.output_format),
    }
}

pub(crate) fn load_word_list(text: &str) -> Result<WordList, CliError> {
    if is_frequency_word_list(text) {
        let entries = parse_frequency_word_list(text).map_err(CliError::CompileFrequencyList)?;
        return WordList::new(entries.into_iter().map(|(word, _)| word))
            .map_err(CliError::InvalidDictionary);
    }
    Ok(WordList::from_text(Normalization::Exact, text))
}

pub(crate) fn load_checker(
    dictionary_paths: &[PathBuf],
    compiled_paths: &[PathBuf],
    hunspell_affix_paths: &[PathBuf],
) -> Result<Checker, CliError> {
    let mut builder = Checker::builder();
    let user_dictionaries = load_user_dictionaries()?;
    if user_dictionaries.is_empty()
        && dictionary_paths.is_empty()
        && compiled_paths.is_empty()
        && hunspell_affix_paths.is_empty()
    {
        return Err(CliError::Usage(
            "check requires a dictionary option or a workspace/global user dictionary".to_owned(),
        ));
    }
    for dictionary in user_dictionaries {
        builder = builder.dictionary(dictionary);
    }
    for path in dictionary_paths {
        let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
            path: path.clone(),
            source,
        })?;
        builder = builder.dictionary(load_word_list(&text)?);
    }
    for path in compiled_paths {
        builder = builder.dictionary(load_artifact(path)?);
    }
    for aff_path in hunspell_affix_paths {
        builder = builder.dictionary(load_installed_hunspell_dictionary(aff_path)?);
    }

    Ok(builder.build())
}

/// Dictionary inputs retained both for recognition and bounded suggestions.
pub(crate) struct AnalysisDictionary {
    pub(crate) sources: Vec<AnalysisSource>,
}

impl AnalysisDictionary {
    fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    fn replacement_rules(&self) -> Vec<ReplacementRule> {
        self.sources
            .iter()
            .flat_map(|source| match source {
                AnalysisSource::WordList(_) => Vec::new(),
                AnalysisSource::Artifact(dictionary) => dictionary.replacement_rules(),
                AnalysisSource::Hunspell(dictionary) => dictionary.replacement_rules().to_vec(),
            })
            .collect()
    }

    pub(crate) fn hunspell_ranking_dictionary(&self) -> Option<&HunspellDictionary> {
        self.sources.iter().find_map(|source| match source {
            AnalysisSource::WordList(_) => None,
            AnalysisSource::Artifact(dictionary) => dictionary.hunspell_dictionary(),
            AnalysisSource::Hunspell(dictionary) => Some(dictionary.as_ref()),
        })
    }

    pub(crate) fn normalize_suggestion_output(&self, candidate: &str) -> String {
        for source in &self.sources {
            match source {
                AnalysisSource::WordList(dictionary) if dictionary.contains(candidate) => {
                    return candidate.to_owned();
                }
                AnalysisSource::Artifact(dictionary)
                    if dictionary.contains(candidate)
                        && dictionary.is_suggestion_candidate(candidate) =>
                {
                    return dictionary.normalize_suggestion_output(candidate);
                }
                AnalysisSource::Hunspell(dictionary)
                    if dictionary.is_suggestion_candidate(candidate) =>
                {
                    return dictionary.normalize_output(candidate);
                }
                AnalysisSource::WordList(_)
                | AnalysisSource::Artifact(_)
                | AnalysisSource::Hunspell(_) => {}
            }
        }
        candidate.to_owned()
    }
}

pub(crate) enum AnalysisSource {
    WordList(WordList),
    Artifact(ArtifactDictionary),
    Hunspell(Box<HunspellDictionary>),
}

impl Dictionary for AnalysisDictionary {
    fn contains(&self, word: &str) -> bool {
        self.sources.iter().any(|source| match source {
            AnalysisSource::WordList(dictionary) => dictionary.contains(word),
            AnalysisSource::Artifact(dictionary) => dictionary.contains(word),
            AnalysisSource::Hunspell(dictionary) => dictionary.contains(word),
        })
    }

    fn as_candidate_source(&self) -> Option<&dyn CandidateSource> {
        Some(self)
    }
}

impl CandidateSource for AnalysisDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for source in &self.sources {
            let mut keep_going = true;
            match source {
                AnalysisSource::WordList(dictionary) => dictionary.visit_candidates(&mut |word| {
                    keep_going = visitor(word);
                    keep_going
                }),
                AnalysisSource::Artifact(dictionary) => dictionary.visit_candidates(&mut |word| {
                    keep_going = visitor(word);
                    keep_going
                }),
                AnalysisSource::Hunspell(dictionary) => dictionary.visit_candidates(&mut |word| {
                    keep_going = visitor(word);
                    keep_going
                }),
            }
            if !keep_going {
                break;
            }
        }
    }

    fn contains_candidate(&self, word: &str) -> bool {
        self.contains(word)
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        for source in &self.sources {
            let mut keep_going = true;
            {
                let mut visit = |word: &str| {
                    keep_going = visitor(word);
                    keep_going
                };
                match source {
                    AnalysisSource::WordList(dictionary) => dictionary.visit_nearby_candidates(
                        query,
                        max_edit_distance,
                        max_word_scalars,
                        &mut visit,
                    ),
                    AnalysisSource::Artifact(dictionary) => dictionary.visit_nearby_candidates(
                        query,
                        max_edit_distance,
                        max_word_scalars,
                        &mut visit,
                    ),
                    AnalysisSource::Hunspell(dictionary) => dictionary.visit_nearby_candidates(
                        query,
                        max_edit_distance,
                        max_word_scalars,
                        &mut visit,
                    ),
                }
            }
            if !keep_going {
                break;
            }
        }
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        self.sources.iter().any(|source| match source {
            AnalysisSource::WordList(dictionary) => dictionary.contains(candidate),
            AnalysisSource::Artifact(dictionary) => {
                dictionary.contains(candidate) && dictionary.is_suggestion_candidate(candidate)
            }
            AnalysisSource::Hunspell(dictionary) => dictionary.is_suggestion_candidate(candidate),
        })
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        self.sources.iter().find_map(|source| match source {
            AnalysisSource::WordList(_) => None,
            AnalysisSource::Artifact(dictionary) => dictionary.candidate_frequency(candidate),
            AnalysisSource::Hunspell(dictionary) => dictionary.candidate_frequency(candidate),
        })
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        for source in &self.sources {
            let mut keep_going = true;
            match source {
                AnalysisSource::WordList(_) => {}
                AnalysisSource::Artifact(dictionary) => dictionary.visit_related_candidates(
                    query,
                    seed,
                    max_edit_distance,
                    &mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    },
                ),
                AnalysisSource::Hunspell(dictionary) => dictionary.visit_related_candidates(
                    query,
                    seed,
                    max_edit_distance,
                    &mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    },
                ),
            }
            if !keep_going {
                break;
            }
        }
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for source in &self.sources {
            let mut keep_going = true;
            match source {
                AnalysisSource::WordList(_) => {}
                AnalysisSource::Artifact(dictionary) => {
                    dictionary.visit_related_seeds(&mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    });
                }
                AnalysisSource::Hunspell(dictionary) => {
                    dictionary.visit_related_seeds(&mut |word| {
                        keep_going = visitor(word);
                        keep_going
                    });
                }
            }
            if !keep_going {
                break;
            }
        }
    }
}

pub(crate) fn load_analysis_dictionary(
    dictionary_paths: &[PathBuf],
    compiled_paths: &[PathBuf],
    hunspell_affix_paths: &[PathBuf],
) -> Result<AnalysisDictionary, CliError> {
    let mut sources = load_user_dictionaries()?
        .into_iter()
        .map(AnalysisSource::WordList)
        .collect::<Vec<_>>();
    for path in dictionary_paths {
        let text = fs::read_to_string(path).map_err(|source| CliError::ReadDictionary {
            path: path.clone(),
            source,
        })?;
        sources.push(AnalysisSource::WordList(load_word_list(&text)?));
    }
    for path in compiled_paths {
        sources.push(AnalysisSource::Artifact(load_artifact(path)?));
    }
    for aff_path in hunspell_affix_paths {
        sources.push(AnalysisSource::Hunspell(Box::new(
            load_installed_hunspell_dictionary(aff_path)?,
        )));
    }
    Ok(AnalysisDictionary { sources })
}

/// A standalone `--compiled` artifact, independent of its source-pair files.
pub(crate) enum ArtifactDictionary {
    Exact(CompiledDictionary),
    Hunspell(Box<HunspellDictionary>),
}

impl ArtifactDictionary {
    fn replacement_rules(&self) -> Vec<ReplacementRule> {
        match self {
            Self::Exact(_) => Vec::new(),
            Self::Hunspell(dictionary) => dictionary.replacement_rules().to_vec(),
        }
    }

    fn hunspell_dictionary(&self) -> Option<&HunspellDictionary> {
        match self {
            Self::Exact(_) => None,
            Self::Hunspell(dictionary) => Some(dictionary.as_ref()),
        }
    }

    fn normalize_suggestion_output(&self, candidate: &str) -> String {
        match self {
            Self::Exact(_) => candidate.to_owned(),
            Self::Hunspell(dictionary) => dictionary.normalize_output(candidate),
        }
    }
}

impl Dictionary for ArtifactDictionary {
    fn contains(&self, word: &str) -> bool {
        match self {
            Self::Exact(dictionary) => dictionary.contains(word),
            Self::Hunspell(dictionary) => dictionary.contains(word),
        }
    }

    fn as_candidate_source(&self) -> Option<&dyn CandidateSource> {
        Some(self)
    }
}

impl CandidateSource for ArtifactDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        match self {
            Self::Exact(dictionary) => dictionary.visit_candidates(visitor),
            Self::Hunspell(dictionary) => dictionary.visit_candidates(visitor),
        }
    }

    fn contains_candidate(&self, word: &str) -> bool {
        self.contains(word)
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        match self {
            Self::Exact(dictionary) => dictionary.visit_nearby_candidates(
                query,
                max_edit_distance,
                max_word_scalars,
                visitor,
            ),
            Self::Hunspell(dictionary) => dictionary.visit_nearby_candidates(
                query,
                max_edit_distance,
                max_word_scalars,
                visitor,
            ),
        }
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        match self {
            Self::Exact(dictionary) => dictionary.frequency(candidate),
            Self::Hunspell(dictionary) => dictionary.candidate_frequency(candidate),
        }
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        match self {
            Self::Exact(_) => true,
            Self::Hunspell(dictionary) => dictionary.is_suggestion_candidate(candidate),
        }
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        if let Self::Hunspell(dictionary) = self {
            dictionary.visit_related_candidates(query, seed, max_edit_distance, visitor);
        }
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        if let Self::Hunspell(dictionary) = self {
            dictionary.visit_related_seeds(visitor);
        }
    }
}

pub(crate) fn load_artifact(path: &Path) -> Result<ArtifactDictionary, CliError> {
    let bytes = read_compiled_artifact(path)?;
    if is_runtime_artifact(&bytes) {
        load_runtime_artifact(&bytes)
            .map(|dictionary| ArtifactDictionary::Hunspell(Box::new(dictionary)))
            .map_err(|source| CliError::LoadHunspellArtifact {
                path: path.to_path_buf(),
                source,
            })
    } else {
        CompiledDictionary::load(bytes)
            .map(ArtifactDictionary::Exact)
            .map_err(|source| CliError::LoadArtifact {
                path: path.to_path_buf(),
                source,
            })
    }
}

pub(crate) fn load_installed_hunspell_dictionary(
    aff_path: &Path,
) -> Result<HunspellDictionary, CliError> {
    let dic_path = aff_path.with_extension("dic");
    let cache_path = runtime_cache_path(aff_path);
    let aff_bytes = fs::read(aff_path).map_err(|source| CliError::ReadInput {
        path: aff_path.to_path_buf(),
        source,
    })?;
    let dic_bytes = fs::read(&dic_path).map_err(|source| CliError::ReadInput {
        path: dic_path.clone(),
        source,
    })?;
    let cache = match fs::read(&cache_path) {
        Ok(cache) => cache,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "notice: no Hunspell runtime cache found at `{}`; importing `{}` and `{}` directly (slower)",
                cache_path.display(),
                aff_path.display(),
                dic_path.display()
            );
            eprintln!(
                "hint: for repeated use or read-only source directories, run `ferrolex compile {} {} -o dictionary.flexh` and pass `--compiled dictionary.flexh`; catalog dictionaries can use `ferrolex dictionary install`",
                aff_path.display(),
                dic_path.display()
            );
            let aff_source = aff_path.display().to_string();
            let dic_source = dic_path.display().to_string();
            let imported = match import_hunspell_source_bytes(
                &aff_source,
                &aff_bytes,
                &dic_source,
                &dic_bytes,
                None,
                ImportMode::Strict,
            ) {
                Ok(imported) => imported,
                Err(source) => {
                    for diagnostic in source.diagnostics() {
                        print_import_diagnostic_to_stderr(diagnostic);
                    }
                    return Err(CliError::ImportHunspellSources {
                        aff_path: aff_path.to_path_buf(),
                        dic_path: dic_path.clone(),
                        source,
                    });
                }
            };
            for diagnostic in imported.diagnostics() {
                print_import_diagnostic_to_stderr(diagnostic);
            }
            return Ok(imported.into_dictionary());
        }
        Err(source) => {
            return Err(CliError::ReadHunspellCache {
                path: cache_path.clone(),
                source,
            });
        }
    };

    load_runtime_cache(
        &cache,
        SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes),
    )
    .map_err(|source| CliError::LoadHunspellCache {
        path: cache_path,
        source,
    })
}

pub(crate) fn check_word(checker: &Checker, word: &str, output_format: OutputFormat) -> RunOutcome {
    let accepted = contains_normalized(checker, word);
    if output_format == OutputFormat::Json {
        print_json(json!({
            "type": "word",
            "command": "check",
            "word": word,
            "status": if accepted { "accepted" } else { "misspelled" },
        }));
    } else if accepted {
        println!("accepted: {word}");
    } else {
        println!("misspelled: {word}");
    }

    if accepted {
        RunOutcome::Success
    } else {
        RunOutcome::Misspelled
    }
}

pub(crate) fn check_inputs(
    checker: &Checker,
    inputs: &[CheckInput],
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let mut outcome = RunOutcome::Success;

    for input in inputs {
        let input_outcome = match input {
            CheckInput::File(path) => check_file(checker, path, output_format)?,
            CheckInput::Stdin => check_stdin(checker, output_format)?,
        };
        if input_outcome == RunOutcome::Misspelled {
            outcome = RunOutcome::Misspelled;
        }
    }

    Ok(outcome)
}

pub(crate) fn check_file(
    checker: &Checker,
    path: &Path,
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let text = fs::read_to_string(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(check_source(checker, path, &text, output_format))
}

pub(crate) fn check_stdin(
    checker: &Checker,
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let path = Path::new("-");
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|source| CliError::ReadInput {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(check_source(checker, path, &text, output_format))
}

pub(crate) fn check_source(
    checker: &Checker,
    path: &Path,
    text: &str,
    output_format: OutputFormat,
) -> RunOutcome {
    let mut misspelled = false;

    let line_index = LineIndex::new(text);
    for issue in check_text(checker, text) {
        print_finding(
            "check",
            output_format,
            path,
            text,
            &line_index,
            issue.range().start,
            issue.word(),
        );
        misspelled = true;
    }

    if misspelled {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    }
}

pub(crate) fn analyze(command: &AnalyzeCommand) -> Result<RunOutcome, CliError> {
    let project = command
        .config_path
        .as_ref()
        .map(|config_path| {
            let text =
                fs::read_to_string(config_path).map_err(|source| CliError::ReadProjectConfig {
                    path: config_path.clone(),
                    source,
                })?;
            ProjectConfig::from_text(&text).map_err(|source| CliError::ProjectConfig {
                path: config_path.clone(),
                source,
            })
        })
        .transpose()?;
    let mut dictionary_paths = command.dictionary_paths.clone();
    let mut compiled_paths = command.compiled_paths.clone();
    let mut hunspell_paths = command.hunspell_affix_paths.clone();
    if let Some(project) = &project {
        let base = command
            .config_path
            .as_ref()
            .and_then(|path| path.parent())
            .unwrap_or(Path::new("."));
        dictionary_paths.extend(project.dictionary_paths().map(|path| base.join(path)));
        compiled_paths.extend(
            project
                .compiled_dictionary_paths()
                .map(|path| base.join(path)),
        );
        hunspell_paths.extend(project.hunspell_paths().map(|path| base.join(path)));
    }
    let dictionary = load_analysis_dictionary(&dictionary_paths, &compiled_paths, &hunspell_paths)?;
    if dictionary.is_empty() {
        return Err(CliError::Usage(
            "analyze requires a dictionary option, configured source, or a workspace/global user dictionary"
                .to_owned(),
        ));
    }
    let mut builder = Analyzer::builder(&dictionary);
    let mut include_patterns = command.include_patterns.clone();
    let mut exclude_patterns = command.exclude_patterns.clone();
    let project_comment_syntax = project.as_ref().and_then(ProjectConfig::comment_syntax);
    if let Some(config) = &project {
        builder =
            builder
                .project_config(config)
                .map_err(|source| CliError::ApplyProjectConfig {
                    path: command
                        .config_path
                        .clone()
                        .expect("project has a config path"),
                    source,
                })?;
        include_patterns.extend(config.include_patterns().map(str::to_owned));
        exclude_patterns.extend(config.exclude_patterns().map(str::to_owned));
    }
    let analyzer = builder.build();
    let paths = analysis_paths(&command.path, &include_patterns, &exclude_patterns)?;
    let mut suggestion_engine = analysis_suggestion_engine(command.suggest, &dictionary);
    let mut has_diagnostic = false;
    for path in paths {
        let Some(source) = read_analysis_source(&path)? else {
            continue;
        };
        let line_index = LineIndex::new(&source);
        let document = analysis_document(
            &source,
            &path,
            command.comment_syntax.as_ref(),
            project_comment_syntax.as_ref(),
        );
        let analysis = analyzer.check(&document);
        for finding in analysis.findings() {
            print_analysis_finding(
                command.output_format,
                &path,
                &source,
                &line_index,
                finding,
                suggestion_engine.as_mut(),
            );
            has_diagnostic = true;
        }
        for diagnostic in analysis.directive_diagnostics() {
            print_directive_diagnostic(
                command.output_format,
                &path,
                &source,
                &line_index,
                diagnostic,
            );
            has_diagnostic = true;
        }
    }
    Ok(if has_diagnostic {
        RunOutcome::Misspelled
    } else {
        RunOutcome::Success
    })
}

pub(crate) fn print_directive_diagnostic(
    output_format: OutputFormat,
    path: &Path,
    source: &str,
    line_index: &LineIndex,
    diagnostic: &ferrolex_code::DirectiveDiagnostic,
) {
    let (line, column) = line_index.line_and_column(source, diagnostic.range().start);
    match output_format {
        OutputFormat::Text => println!(
            "{}:{line}:{column}: malformed directive: {:?}",
            path.display(),
            diagnostic.problem()
        ),
        OutputFormat::Json => print_json(json!({
            "type": "finding",
            "kind": "directive",
            "command": "analyze",
            "path": path.display().to_string(),
            "line": line,
            "column": column,
            "problem": directive_problem_code(diagnostic.problem()),
        })),
    }
}

pub(crate) fn read_analysis_source(path: &Path) -> Result<Option<String>, CliError> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(source) if source.kind() == io::ErrorKind::InvalidData => {
            eprintln!("warning: skipping non-UTF-8 input '{}'", path.display());
            Ok(None)
        }
        Err(source) => Err(CliError::ReadInput {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn analysis_document<'source>(
    source: &'source str,
    path: &Path,
    command_syntax: Option<&CommentSyntax>,
    project_syntax: Option<&CommentSyntax>,
) -> Document<'source> {
    let syntax = command_syntax
        .or(project_syntax)
        .cloned()
        .unwrap_or_else(|| comment_syntax_for_path(path));
    Document::new(source).with_comment_syntax(syntax)
}

pub(crate) fn comment_syntax_for_path(path: &Path) -> CommentSyntax {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(
            "rs" | "c" | "cc" | "cpp" | "h" | "hpp" | "java" | "js" | "jsx" | "ts" | "tsx" | "go"
            | "swift" | "kt",
        ) => CommentSyntax::line("//"),
        Some("py" | "rb" | "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml") => {
            CommentSyntax::line("#")
        }
        Some("sql" | "lua" | "hs") => CommentSyntax::line("--"),
        Some("md" | "markdown" | "html" | "htm" | "xml") => CommentSyntax::Html,
        _ => CommentSyntax::None,
    }
}

pub(crate) fn analysis_paths(
    path: &Path,
    include_patterns: &[String],
    exclude_patterns: &[String],
) -> Result<Vec<PathBuf>, CliError> {
    let metadata = fs::metadata(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut paths = Vec::new();
    collect_analysis_paths(path, path, include_patterns, exclude_patterns, &mut paths)?;
    paths.sort();
    Ok(paths)
}

pub(crate) fn collect_analysis_paths(
    root: &Path,
    directory: &Path,
    includes: &[String],
    excludes: &[String],
    paths: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| CliError::ReadInput {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CliError::ReadInput {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let file_type = entry.file_type().map_err(|source| CliError::ReadInput {
            path: path.clone(),
            source,
        })?;
        if matches_any(&relative, excludes)
            || (file_type.is_dir() && is_vcs_metadata_directory(&path))
        {
            continue;
        }
        if file_type.is_dir() {
            collect_analysis_paths(root, &path, includes, excludes, paths)?;
        } else if file_type.is_file() && (includes.is_empty() || matches_any(&relative, includes)) {
            paths.push(path);
        }
    }
    Ok(())
}

pub(crate) fn matches_any(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_matches(pattern, path))
}

pub(crate) fn is_vcs_metadata_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | ".hg" | ".svn"))
}

pub(crate) fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_matches_bytes(pattern.as_bytes(), path.as_bytes())
}

pub(crate) fn glob_matches_bytes(pattern: &[u8], path: &[u8]) -> bool {
    match (pattern, path) {
        ([], []) => true,
        ([b'*', b'*', b'/', rest @ ..], _) => glob_starstar_directory(pattern, rest, path),
        ([b'*', b'*', rest @ ..], _) => glob_starstar(pattern, rest, path),
        ([b'*', rest @ ..], _) => {
            glob_matches_bytes(rest, path)
                || (!path.is_empty() && path[0] != b'/' && glob_matches_bytes(pattern, &path[1..]))
        }
        ([b'?', rest @ ..], [_, path_rest @ ..]) => glob_matches_bytes(rest, path_rest),
        ([first, rest @ ..], [candidate, path_rest @ ..]) if first == candidate => {
            glob_matches_bytes(rest, path_rest)
        }
        _ => false,
    }
}

pub(crate) fn glob_starstar(pattern: &[u8], rest: &[u8], path: &[u8]) -> bool {
    glob_matches_bytes(rest, path) || (!path.is_empty() && glob_matches_bytes(pattern, &path[1..]))
}

pub(crate) fn glob_starstar_directory(pattern: &[u8], rest: &[u8], path: &[u8]) -> bool {
    glob_starstar(pattern, rest, path)
}

pub(crate) fn print_analysis_finding(
    output_format: OutputFormat,
    path: &Path,
    source: &str,
    line_index: &LineIndex,
    finding: &ferrolex_code::Finding<'_>,
    suggestion_engine: Option<&mut AnalysisSuggestionEngine<'_, AnalysisDictionary>>,
) {
    let (line, column) = line_index.line_and_column(source, finding.range().start);
    let suggestions = suggestion_engine.map_or_else(Vec::new, |engine| engine.suggestions(finding));

    match output_format {
        OutputFormat::Text => {
            print_finding(
                "analyze",
                OutputFormat::Text,
                path,
                source,
                line_index,
                finding.range().start,
                finding.word(),
            );
            for (replacement, distance) in suggestions {
                println!(
                    "{}:{line}:{column}: suggestion: {replacement} (distance {distance})",
                    path.display()
                );
            }
        }
        OutputFormat::Json => {
            let suggestions = suggestions
                .into_iter()
                .map(|(word, distance)| json!({ "word": word, "distance": distance }))
                .collect::<Vec<_>>();
            print_json(json!({
                "type": "finding",
                "kind": "spelling",
                "command": "analyze",
                "path": path.display().to_string(),
                "line": line,
                "column": column,
                "word": finding.word(),
                "suggestions": suggestions,
            }));
        }
    }
}

pub(crate) struct AnalysisSuggestionEngine<'source, S: ?Sized> {
    pub(crate) suggester: Suggester<'source, S>,
    pub(crate) scratch: SuggestScratch,
    pub(crate) output: Vec<Suggestion>,
    pub(crate) cache: HashMap<String, Vec<(String, usize)>>,
}

pub(crate) fn analysis_suggestion_engine(
    include_suggestions: bool,
    dictionary: &AnalysisDictionary,
) -> Option<AnalysisSuggestionEngine<'_, AnalysisDictionary>> {
    include_suggestions.then(|| AnalysisSuggestionEngine::new(dictionary))
}

impl<'source, S: CandidateSource + ?Sized> AnalysisSuggestionEngine<'source, S> {
    pub(crate) fn new(source: &'source S) -> Self {
        let config = SuggestConfig {
            max_results: 3,
            ..SuggestConfig::default()
        };
        Self {
            suggester: Suggester::new(source, config),
            scratch: SuggestScratch::default(),
            output: Vec::new(),
            cache: HashMap::new(),
        }
    }

    pub(crate) fn suggestions(
        &mut self,
        finding: &ferrolex_code::Finding<'_>,
    ) -> Vec<(String, usize)> {
        self.base_suggestions(finding.word())
            .into_iter()
            .map(|(word, distance)| {
                let replacement = finding.whole_identifier_suggestion(&word).unwrap_or(word);
                (replacement, distance)
            })
            .collect()
    }

    pub(crate) fn base_suggestions(&mut self, word: &str) -> Vec<(String, usize)> {
        if let Some(suggestions) = self.cache.get(word) {
            return suggestions.clone();
        }

        self.suggester
            .suggest_into(word, &mut self.output, &mut self.scratch);
        let suggestions = self
            .output
            .iter()
            .map(|suggestion| (suggestion.word().to_owned(), suggestion.distance()))
            .collect::<Vec<_>>();
        if self.cache.len() < MAX_ANALYSIS_SUGGESTION_CACHE_ENTRIES {
            self.cache.insert(word.to_owned(), suggestions.clone());
        }
        suggestions
    }
}

pub(crate) const fn directive_problem_code(problem: DirectiveProblem) -> &'static str {
    match problem {
        DirectiveProblem::MissingIgnoredWords => "missing-ignored-words",
        DirectiveProblem::UnexpectedArguments => "unexpected-arguments",
        DirectiveProblem::UnknownDirective => "unknown-directive",
        _ => "unsupported",
    }
}

pub(crate) fn validate(command: &ValidateCommand) -> Result<RunOutcome, CliError> {
    match command {
        ValidateCommand::Hunspell {
            strict,
            aff_path,
            dic_path,
            output_format,
        } => validate_hunspell(*strict, aff_path, dic_path, None, *output_format),
        ValidateCommand::Compiled {
            path,
            output_format,
        } => validate_compiled(path, *output_format),
    }
}

pub(crate) fn validate_hunspell(
    strict: bool,
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    let (import, _) = import_hunspell_files(aff_path, dic_path, encodings, strict)?;
    Ok(report_hunspell_import(import, dic_path, output_format))
}

pub(crate) fn install_hunspell_runtime_cache(
    locale: &str,
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
) -> Result<RunOutcome, CliError> {
    eprintln!("importing Hunspell sources for {locale}...");
    let (import, sources) = import_hunspell_files(aff_path, dic_path, encodings, true)?;
    let result = match import {
        Ok(result) => result,
        Err(error) => {
            for diagnostic in error.diagnostics() {
                print_import_diagnostic(diagnostic);
            }
            return Ok(RunOutcome::Misspelled);
        }
    };
    for diagnostic in result.diagnostics() {
        print_import_diagnostic(diagnostic);
    }
    eprintln!("building runtime cache for {locale}...");
    let cache = compile_runtime_cache(result.dictionary(), sources)
        .map_err(CliError::CompileHunspellCache)?;
    let cache_path = runtime_cache_path(aff_path);
    eprintln!(
        "writing runtime cache for {locale} to {}...",
        cache_path.display()
    );
    atomic_write_runtime_cache(&cache_path, &cache)?;
    println!("valid: {}", dic_path.display());
    println!("runtime-cache: {}", cache_path.display());
    println!("ready: {locale}");
    Ok(RunOutcome::Success)
}

pub(crate) fn runtime_cache_path(aff_path: &Path) -> PathBuf {
    aff_path.with_extension(HUNSPELL_RUNTIME_CACHE_EXTENSION)
}

pub(crate) fn import_hunspell_files(
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
    strict: bool,
) -> Result<(Result<ImportResult, ImportError>, SourceDigests), CliError> {
    let aff_bytes = fs::read(aff_path).map_err(|source| CliError::ReadInput {
        path: aff_path.to_path_buf(),
        source,
    })?;
    let dic_bytes = fs::read(dic_path).map_err(|source| CliError::ReadInput {
        path: dic_path.to_path_buf(),
        source,
    })?;
    let mode = if strict {
        ImportMode::Strict
    } else {
        ImportMode::Lenient
    };
    let aff_source = aff_path.display().to_string();
    let dic_source = dic_path.display().to_string();

    let import = import_hunspell_source_bytes(
        &aff_source,
        &aff_bytes,
        &dic_source,
        &dic_bytes,
        encodings,
        mode,
    );

    Ok((
        import,
        SourceDigests::from_source_bytes(&aff_bytes, &dic_bytes),
    ))
}

pub(crate) fn import_hunspell_source_bytes(
    aff_source: &str,
    aff_bytes: &[u8],
    dic_source: &str,
    dic_bytes: &[u8],
    encodings: Option<ByteImportEncodings>,
    mode: ImportMode,
) -> Result<ImportResult, ImportError> {
    match encodings {
        Some(encodings) => import_hunspell_bytes_with_encodings(
            aff_source, aff_bytes, dic_source, dic_bytes, encodings, mode,
        ),
        None => import_hunspell_bytes(aff_source, aff_bytes, dic_source, dic_bytes, mode),
    }
}

pub(crate) fn report_hunspell_import(
    import: Result<ImportResult, ImportError>,
    dic_path: &Path,
    output_format: OutputFormat,
) -> RunOutcome {
    match import {
        Ok(result) => {
            let has_errors = result
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity() == Severity::Error);
            for diagnostic in result.diagnostics() {
                print_import_diagnostic_with_format(diagnostic, output_format);
            }
            if has_errors {
                print_validation_result(dic_path, false, output_format);
                RunOutcome::Misspelled
            } else {
                print_validation_result(dic_path, true, output_format);
                RunOutcome::Success
            }
        }
        Err(error) => {
            for diagnostic in error.diagnostics() {
                print_import_diagnostic_with_format(diagnostic, output_format);
            }
            print_validation_result(dic_path, false, output_format);
            RunOutcome::Misspelled
        }
    }
}

pub(crate) fn print_validation_result(path: &Path, valid: bool, output_format: OutputFormat) {
    match output_format {
        OutputFormat::Text if valid => println!("valid: {}", path.display()),
        OutputFormat::Text => {}
        OutputFormat::Json => print_json(json!({
            "type": "validation",
            "path": path.display().to_string(),
            "status": if valid { "valid" } else { "invalid" },
        })),
    }
}

pub(crate) fn atomic_write_runtime_cache(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    sweep_stale_temporary_siblings(path);
    let temporary = temporary_sibling(path);
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| CliError::WriteHunspellCache {
                path: temporary.clone(),
                source,
            })?;
        created = true;
        file.write_all(bytes)
            .map_err(|source| CliError::WriteHunspellCache {
                path: temporary.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| CliError::WriteHunspellCache {
                path: temporary.clone(),
                source,
            })?;
        fs::rename(&temporary, path).map_err(|source| CliError::WriteHunspellCache {
            path: path.to_path_buf(),
            source,
        })?;
        sync_parent_directory(parent).map_err(|source| CliError::WriteHunspellCache {
            path: parent.to_path_buf(),
            source,
        })
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn validate_compiled(
    path: &Path,
    output_format: OutputFormat,
) -> Result<RunOutcome, CliError> {
    match load_artifact(path)? {
        ArtifactDictionary::Exact(dictionary) => {
            dictionary
                .validate()
                .map_err(|source| CliError::ValidateArtifact {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
        ArtifactDictionary::Hunspell(_) => {}
    }
    print_validation_result(path, true, output_format);
    Ok(RunOutcome::Success)
}

pub(crate) fn inspect_artifact(path: &Path) -> Result<RunOutcome, CliError> {
    let bytes = read_compiled_artifact(path)?;
    match inspect_compiled_artifact(&bytes) {
        Ok(metadata) => {
            println!("artifact: {}", path.display());
            println!("format: FLEXDIC");
            println!("format-version: {}", metadata.format_version());
            println!("source-metadata: not recorded (plain word-list artifact)");
            println!("format-capabilities: exact-word-lookup");
            println!("feature-bits: {:#x}", metadata.feature_bits());
            println!("entries: {}", metadata.word_count());
        }
        Err(LoadError::InvalidMagic) => {
            let metadata =
                inspect_runtime_cache(&bytes).map_err(|source| CliError::LoadHunspellArtifact {
                    path: path.to_path_buf(),
                    source,
                })?;
            let sources = metadata.sources();
            println!("artifact: {}", path.display());
            println!("format: FLXHSP");
            println!("format-version: {}", metadata.format_version());
            println!("semantics-version: {}", metadata.semantics_version());
            println!("source-aff-sha256: {}", hex_digest(sources.aff()));
            println!("source-dic-sha256: {}", hex_digest(sources.dic()));
            println!(
                "format-capabilities: flag-modes, case-fallback, language-casing, morphology, lexemes, prefixes, suffixes, cross-product, continuation-flags, conditions, special-flags, keyboard-layout, character-maps, compounds, breaks, word-characters, replacement-rules, ignored-characters, input-conversions, output-conversions, full-strip, complex-prefixes"
            );
        }
        Err(source) => {
            return Err(CliError::LoadArtifact {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    Ok(RunOutcome::Success)
}

pub(crate) fn hex_digest(digest: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

pub(crate) fn read_compiled_artifact(path: &Path) -> Result<Vec<u8>, CliError> {
    let size = fs::metadata(path)
        .map_err(|source| CliError::ReadInput {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if size > u64::try_from(MAX_COMPILED_ARTIFACT_BYTES).expect("usize fits u64") {
        return Err(CliError::ArtifactTooLarge {
            path: path.to_path_buf(),
            actual: size,
        });
    }
    fs::read(path).map_err(|source| CliError::ReadInput {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn print_import_diagnostic(diagnostic: &ImportDiagnostic) {
    println!("{}", render_import_diagnostic(diagnostic));
}

pub(crate) fn print_import_diagnostic_with_format(
    diagnostic: &ImportDiagnostic,
    output_format: OutputFormat,
) {
    match output_format {
        OutputFormat::Text => print_import_diagnostic(diagnostic),
        OutputFormat::Json => print_json(json!({
            "type": "diagnostic",
            "command": "validate",
            "source": diagnostic.source(),
            "line": diagnostic.line(),
            "directive": diagnostic.directive(),
            "severity": severity_code(diagnostic.severity()),
            "message": diagnostic.message(),
        })),
    }
}

pub(crate) const fn severity_code(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

pub(crate) fn print_import_diagnostic_to_stderr(diagnostic: &ImportDiagnostic) {
    eprintln!("{}", render_import_diagnostic(diagnostic));
}

pub(crate) fn render_import_diagnostic(diagnostic: &ImportDiagnostic) -> String {
    diagnostic.to_string()
}

pub(crate) fn print_finding(
    command: &str,
    output_format: OutputFormat,
    path: &Path,
    source: &str,
    line_index: &LineIndex,
    byte_offset: usize,
    word: &str,
) {
    let (line, column) = line_index.line_and_column(source, byte_offset);
    match output_format {
        OutputFormat::Text => {
            println!("{}:{line}:{column}: misspelled: {word}", path.display());
        }
        OutputFormat::Json => print_json(json!({
            "type": "finding",
            "kind": "spelling",
            "command": command,
            "path": path.display().to_string(),
            "line": line,
            "column": column,
            "word": word,
        })),
    }
}

pub(crate) fn print_json(value: impl fmt::Display) {
    println!("{value}");
}

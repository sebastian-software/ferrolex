//! Allocating diagnostic explanations for Hunspell recognition.
//!
//! This module intentionally has no callers on the ordinary `contains` path.
//! It is an experimental observability surface, so its owned result can retain
//! a useful derivation without changing lookup's allocation contract.

use super::{
    compound_boundaries, AffixKind, AffixRule, Flag, FormState, HunspellDictionary, Lexeme,
    MAX_AFFIX_CHAIN, MAX_DERIVATIONS_PER_LEXEME,
};

/// An experimental, owned explanation of one dictionary lookup.
///
/// This API is deliberately unstable. It is a diagnostic path rather than a
/// stable serialization format, and callers must not depend on every detail of
/// the internal Hunspell search strategy.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LookupExplanation {
    /// The word was accepted and its matching path is available.
    Accepted(Acceptance),
    /// The word was rejected with the most specific reason known to the
    /// diagnostic path.
    Rejected(Rejection),
}

impl LookupExplanation {
    /// Returns the acceptance details when the lookup succeeded.
    #[must_use]
    pub const fn accepted(&self) -> Option<&Acceptance> {
        match self {
            Self::Accepted(acceptance) => Some(acceptance),
            Self::Rejected(_) => None,
        }
    }

    /// Returns the rejection details when the lookup failed.
    #[must_use]
    pub const fn rejected(&self) -> Option<&Rejection> {
        match self {
            Self::Accepted(_) => None,
            Self::Rejected(rejection) => Some(rejection),
        }
    }
}

/// Details of an accepted lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acceptance {
    casing: CasingPath,
    kind: AcceptanceKind,
}

impl Acceptance {
    /// Returns the input casing path that led to the accepted spelling.
    #[must_use]
    pub const fn casing(&self) -> &CasingPath {
        &self.casing
    }

    /// Returns the recognition path.
    #[must_use]
    pub const fn kind(&self) -> &AcceptanceKind {
        &self.kind
    }
}

/// How capitalization was handled for an accepted lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CasingPath {
    /// The supplied spelling was looked up directly.
    Exact,
    /// A Hunspell capitalization fallback matched this lower- or initial-case
    /// spelling. `candidate` is the spelling supplied to the normal matcher.
    CaseFallback { candidate: String },
}

/// The accepting semantic path.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcceptanceKind {
    /// A stored stem was accepted without a derivation.
    Stem { stem: String },
    /// A stored stem accepted after the listed prefix/suffix transformations.
    Affixed {
        stem: String,
        rules: Vec<AppliedAffix>,
    },
    /// A compound was accepted after splitting it into these components.
    Compound { components: Vec<CompoundComponent> },
    /// A narrow compatibility path accepted the spelling but does not yet have
    /// a richer source-level trace.
    Compatibility { detail: String },
}

/// One prefix or suffix transformation in an affix derivation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedAffix {
    kind: AppliedAffixKind,
    strip: String,
    add: String,
    continuation_flags: Vec<String>,
}

impl AppliedAffix {
    /// Returns whether this was a prefix or suffix transformation.
    #[must_use]
    pub const fn kind(&self) -> AppliedAffixKind {
        self.kind
    }

    /// Returns the source text removed from the previous form.
    #[must_use]
    pub fn strip(&self) -> &str {
        &self.strip
    }

    /// Returns the source text added to the previous form.
    #[must_use]
    pub fn add(&self) -> &str {
        &self.add
    }

    /// Returns the continuation flags made available by this rule.
    #[must_use]
    pub fn continuation_flags(&self) -> &[String] {
        &self.continuation_flags
    }
}

/// The direction of an applied affix rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppliedAffixKind {
    /// The rule added or removed text at the start of the form.
    Prefix,
    /// The rule added or removed text at the end of the form.
    Suffix,
}

/// One component of a recognized compound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompoundComponent {
    spelling: String,
    stem: String,
    role: CompoundComponentRole,
}

impl CompoundComponent {
    /// Returns the slice taken from the queried compound.
    #[must_use]
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// Returns the stored stem that accepted this component.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// Returns the component's rule position.
    #[must_use]
    pub const fn role(&self) -> CompoundComponentRole {
        self.role
    }
}

/// The matching role assigned to a compound component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompoundComponentRole {
    /// A generic compound flag or `COMPOUNDRULE` accepted the component.
    Generic,
    /// The first component selected by `COMPOUNDBEGIN`.
    Begin,
    /// A middle component selected by `COMPOUNDMIDDLE`.
    Middle,
    /// The final component selected by `COMPOUNDEND`.
    End,
}

/// Details of a rejected lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rejection {
    reason: RejectionReason,
}

impl Rejection {
    /// Returns the rejection reason.
    #[must_use]
    pub const fn reason(&self) -> &RejectionReason {
        &self.reason
    }
}

/// The most specific rejection reason the diagnostic path can establish.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RejectionReason {
    /// A matching stored stem is marked `FORBIDDENWORD`.
    ForbiddenStem { stem: String },
    /// A matching stored stem is marked `NEEDAFFIX`.
    NeedsAffix { stem: String },
    /// A matching stored stem is marked `ONLYINCOMPOUND`.
    OnlyInCompound { stem: String },
    /// A matching stored stem requires its exact case.
    KeepCase { stem: String },
    /// No accepted stem, derivation, or compound segmentation was found.
    NoDerivation,
}

struct TracedFormState {
    state: FormState,
    rules: Vec<AppliedAffix>,
}

impl HunspellDictionary {
    /// Explains why `word` was accepted or rejected.
    ///
    /// Unlike [`ferrolex_core::Dictionary::contains`], this experimental
    /// diagnostic API allocates owned result data and replays bounded affix and
    /// compound search to retain a useful path. It never changes the normal
    /// lookup result or hot-path allocation behavior.
    #[must_use]
    pub fn explain(&self, word: &str) -> LookupExplanation {
        let normalized = self.normalize_input(word);
        if let Some(kind) = self.explain_normalized(normalized.as_ref(), true) {
            return LookupExplanation::Accepted(Acceptance {
                casing: CasingPath::Exact,
                kind,
            });
        }
        for candidate in self.case_folded_candidates(normalized.as_ref()) {
            if let Some(kind) = self.explain_normalized(&candidate, false) {
                return LookupExplanation::Accepted(Acceptance {
                    casing: CasingPath::CaseFallback { candidate },
                    kind,
                });
            }
        }
        LookupExplanation::Rejected(Rejection {
            reason: self.rejection_reason(normalized.as_ref(), true),
        })
    }

    fn explain_normalized(&self, word: &str, allow_keep_case: bool) -> Option<AcceptanceKind> {
        self.explain_stem(word, allow_keep_case)
            .or_else(|| self.explain_single_affix(word, allow_keep_case))
            .or_else(|| self.explain_derived(word, allow_keep_case))
            .or_else(|| self.explain_compound(word, allow_keep_case))
            .or_else(|| {
                self.matches_without_break(word, allow_keep_case).then(|| {
                    AcceptanceKind::Compatibility {
                        detail: "accepted by a bounded compatibility lookup path".to_owned(),
                    }
                })
            })
            .or_else(|| {
                self.matches_break_word(word, allow_keep_case).then(|| {
                    AcceptanceKind::Compatibility {
                        detail: "accepted after applying a BREAK pattern".to_owned(),
                    }
                })
            })
            .or_else(|| {
                self.sharp_uppercase_forms
                    .contains(word)
                    .then(|| AcceptanceKind::Compatibility {
                        detail: "accepted by CHECKSHARPS uppercase compatibility".to_owned(),
                    })
            })
    }

    fn explain_stem(&self, word: &str, allow_keep_case: bool) -> Option<AcceptanceKind> {
        self.lexemes_for_stem(word)
            .find(|lexeme| self.accepts_stem(lexeme, allow_keep_case))
            .map(|lexeme| AcceptanceKind::Stem {
                stem: lexeme.stem.to_string(),
            })
    }

    fn accepts_stem(&self, lexeme: &Lexeme, allow_keep_case: bool) -> bool {
        !self.is_forbidden(&lexeme.flags)
            && !self.requires_affix(&lexeme.flags)
            && !self.is_only_in_compound(&lexeme.flags)
            && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
    }

    fn explain_single_affix(&self, word: &str, allow_keep_case: bool) -> Option<AcceptanceKind> {
        for rule in self.prefixes.iter().chain(&self.suffixes) {
            if !rule.could_generate(word) {
                continue;
            }
            let Some(stem) = rule.reverse_apply(word, self.full_strip) else {
                continue;
            };
            if let Some(lexeme) = self.lexemes_for_stem(&stem).find(|lexeme| {
                !self.is_forbidden(&lexeme.flags)
                    && lexeme.flags.contains(&rule.flag)
                    && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                    && self.is_accepted_state(&FormState::new(lexeme).apply(
                        rule,
                        word.to_owned(),
                        &self.special_flags,
                    ))
            }) {
                return Some(AcceptanceKind::Affixed {
                    stem: lexeme.stem.to_string(),
                    rules: vec![applied_affix(rule)],
                });
            }
        }
        None
    }

    fn explain_derived(&self, word: &str, allow_keep_case: bool) -> Option<AcceptanceKind> {
        let candidates = self.derived_candidate_indices(word)?;
        for index in candidates {
            let lexeme = &self.lexemes[index];
            if self.is_forbidden(&lexeme.flags)
                || (!allow_keep_case && self.is_keep_case(&lexeme.flags))
            {
                continue;
            }
            let mut states = vec![TracedFormState {
                state: FormState::new(lexeme),
                rules: Vec::new(),
            }];
            let mut derivations = 0;
            while let Some(traced) = states.pop() {
                if traced.state.depth > 0
                    && traced.state.form == word
                    && self.is_accepted_state(&traced.state)
                {
                    return Some(AcceptanceKind::Affixed {
                        stem: lexeme.stem.to_string(),
                        rules: traced.rules,
                    });
                }
                if traced.state.depth == MAX_AFFIX_CHAIN {
                    continue;
                }
                if !self.expand_traced_rules(
                    &traced,
                    AffixKind::Prefix,
                    &self.prefixes,
                    &self.prefix_rules_by_flag,
                    &mut states,
                    &mut derivations,
                ) || !self.expand_traced_rules(
                    &traced,
                    AffixKind::Suffix,
                    &self.suffixes,
                    &self.suffix_rules_by_flag,
                    &mut states,
                    &mut derivations,
                ) {
                    break;
                }
            }
        }
        None
    }

    fn expand_traced_rules(
        &self,
        traced: &TracedFormState,
        kind: AffixKind,
        rules: &[AffixRule],
        rule_indices_by_flag: &std::collections::BTreeMap<super::Flag, Vec<usize>>,
        states: &mut Vec<TracedFormState>,
        derivations: &mut usize,
    ) -> bool {
        for flag in traced.state.flags_for(kind) {
            let Some(rule_indices) = rule_indices_by_flag.get(flag) else {
                continue;
            };
            for index in rule_indices {
                let rule = &rules[*index];
                if !traced.state.can_apply(rule, self.complex_prefixes) {
                    continue;
                }
                if let Some(form) = rule.apply(&traced.state.form, self.full_strip) {
                    if *derivations == MAX_DERIVATIONS_PER_LEXEME {
                        return false;
                    }
                    *derivations += 1;
                    let mut path = traced.rules.clone();
                    path.push(applied_affix(rule));
                    states.push(TracedFormState {
                        state: traced.state.apply(rule, form, &self.special_flags),
                        rules: path,
                    });
                }
            }
        }
        true
    }

    fn explain_compound(&self, word: &str, allow_keep_case: bool) -> Option<AcceptanceKind> {
        if self.compound.flag.is_none()
            && self.compound.rules.is_empty()
            && (self.compound.begin.is_none() || self.compound.end.is_none())
        {
            return None;
        }
        if self.compound.check_replacement
            && self.matches_noncompound_replacement(word, allow_keep_case)
        {
            return None;
        }
        let boundaries = compound_boundaries(word)?;

        if let Some(flag) = self.compound.flag.as_ref() {
            for count in 2..boundaries.len() {
                if !self.compound_component_count_is_allowed(word, count) {
                    continue;
                }
                if let Some(components) = self.trace_compound_components(
                    word,
                    &boundaries,
                    std::iter::repeat_n((flag, CompoundComponentRole::Generic), count),
                    allow_keep_case,
                ) {
                    return Some(AcceptanceKind::Compound { components });
                }
                if self.compound_component_count_cannot_continue(count) {
                    return None;
                }
            }
        }
        for rule in &self.compound.rules {
            for pattern in &rule.patterns {
                if let Some(components) = self.trace_compound_components(
                    word,
                    &boundaries,
                    pattern
                        .iter()
                        .map(|flag| (flag, CompoundComponentRole::Generic)),
                    allow_keep_case,
                ) {
                    return Some(AcceptanceKind::Compound { components });
                }
            }
        }
        self.trace_positioned_compound(word, &boundaries, allow_keep_case)
            .map(|components| AcceptanceKind::Compound { components })
    }

    fn trace_compound_components<'a>(
        &self,
        word: &str,
        boundaries: &[usize],
        requirements: impl IntoIterator<Item = (&'a super::Flag, CompoundComponentRole)>,
        allow_keep_case: bool,
    ) -> Option<Vec<CompoundComponent>> {
        let requirements = requirements.into_iter().collect::<Vec<_>>();
        let mut components = Vec::with_capacity(requirements.len());
        self.trace_compound_from(
            word,
            boundaries,
            &requirements,
            0,
            0,
            allow_keep_case,
            &mut components,
        )
        .then_some(components)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the diagnostic DFS keeps every compound transition explicit"
    )]
    fn trace_compound_from(
        &self,
        word: &str,
        boundaries: &[usize],
        requirements: &[(&super::Flag, CompoundComponentRole)],
        requirement_index: usize,
        start: usize,
        allow_keep_case: bool,
        components: &mut Vec<CompoundComponent>,
    ) -> bool {
        if requirement_index == requirements.len() {
            return start + 1 == boundaries.len();
        }
        let (required_flag, role) = requirements[requirement_index];
        let is_final = requirement_index + 1 == requirements.len();
        let first_end = start.saturating_add(self.compound.minimum_length);
        for end in first_end..boundaries.len() {
            let candidate = &word[boundaries[start]..boundaries[end]];
            if !self.compound_boundary_is_allowed(word, boundaries[start], boundaries[end], false) {
                continue;
            }
            let Some(lexeme) = self.lexemes_for_stem(candidate).find(|lexeme| {
                !self.is_forbidden(&lexeme.flags)
                    && (is_final || !self.is_compound_forbidden(&lexeme.flags))
                    && lexeme.flags.contains(required_flag)
                    && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
            }) else {
                continue;
            };
            components.push(CompoundComponent {
                spelling: candidate.to_owned(),
                stem: lexeme.stem.to_string(),
                role,
            });
            if self.trace_compound_from(
                word,
                boundaries,
                requirements,
                requirement_index + 1,
                end,
                allow_keep_case,
                components,
            ) {
                return true;
            }
            components.pop();
        }
        false
    }

    fn trace_positioned_compound(
        &self,
        word: &str,
        boundaries: &[usize],
        allow_keep_case: bool,
    ) -> Option<Vec<CompoundComponent>> {
        let (Some(begin), Some(end)) = (&self.compound.begin, &self.compound.end) else {
            return None;
        };
        for count in 2..boundaries.len() {
            if !self.compound_component_count_is_allowed(word, count) {
                continue;
            }
            let mut requirements = Vec::with_capacity(count);
            requirements.push((begin, CompoundComponentRole::Begin));
            if count > 2 {
                let middle = self.compound.middle.as_ref()?;
                requirements.extend(std::iter::repeat_n(
                    (middle, CompoundComponentRole::Middle),
                    count - 2,
                ));
            }
            requirements.push((end, CompoundComponentRole::End));
            if let Some(components) =
                self.trace_compound_components(word, boundaries, requirements, allow_keep_case)
            {
                return Some(components);
            }
            if self.compound_component_count_cannot_continue(count) {
                return None;
            }
        }
        None
    }

    fn rejection_reason(&self, word: &str, allow_keep_case: bool) -> RejectionReason {
        for lexeme in self.lexemes_for_stem(word) {
            if self.is_forbidden(&lexeme.flags) {
                return RejectionReason::ForbiddenStem {
                    stem: lexeme.stem.to_string(),
                };
            }
            if self.requires_affix(&lexeme.flags) {
                return RejectionReason::NeedsAffix {
                    stem: lexeme.stem.to_string(),
                };
            }
            if self.is_only_in_compound(&lexeme.flags) {
                return RejectionReason::OnlyInCompound {
                    stem: lexeme.stem.to_string(),
                };
            }
            if !allow_keep_case && self.is_keep_case(&lexeme.flags) {
                return RejectionReason::KeepCase {
                    stem: lexeme.stem.to_string(),
                };
            }
        }
        RejectionReason::NoDerivation
    }
}

fn applied_affix(rule: &AffixRule) -> AppliedAffix {
    AppliedAffix {
        kind: match rule.kind {
            AffixKind::Prefix => AppliedAffixKind::Prefix,
            AffixKind::Suffix => AppliedAffixKind::Suffix,
        },
        strip: rule.strip.to_string(),
        add: rule.add.to_string(),
        continuation_flags: rule.continuation_flags.iter().map(flag_label).collect(),
    }
}

fn flag_label(flag: &Flag) -> String {
    match flag {
        Flag::Numeric(value) => value.to_string(),
        Flag::Text(value) => value.to_string(),
    }
}

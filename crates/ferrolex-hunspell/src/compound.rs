//! Bounded compound recognition and suggestion expansion.

use super::{
    compound_boundaries, has_flag, has_triple_at_compound_boundary, CompoundPosition, Flag,
    FormState, HunspellDictionary, MAX_COMPOUND_PATTERN_REPLACEMENT_VARIANTS, MAX_COMPOUND_SCALARS,
};

impl HunspellDictionary {
    /// Evaluates the bounded compound-recognition entry point.
    ///
    /// This dispatches `COMPOUNDFLAG`, `COMPOUNDRULE`, positioned
    /// `COMPOUNDBEGIN`/`COMPOUNDMIDDLE`/`COMPOUNDEND`, and the replacement and
    /// triple safeguards described in `docs/compound-semantics.md`. The input
    /// is first reduced to Unicode-scalar boundaries so every downstream DP
    /// transition shares the 256-scalar query limit.
    pub(crate) fn matches_simple_compound(&self, word: &str, allow_keep_case: bool) -> bool {
        if self.compound.flag.is_none()
            && self.compound.rules.is_empty()
            && (self.compound.begin.is_none() || self.compound.end.is_none())
        {
            return false;
        }
        if self.compound.check_replacement
            && self.matches_noncompound_replacement(word, allow_keep_case)
        {
            return false;
        }
        // Retain at most the bounded number of split positions. Building an
        // index for an arbitrarily long untrusted query would defeat the
        // compound-evaluation limit before it can reject the query.
        let mut boundaries = word
            .char_indices()
            .take(MAX_COMPOUND_SCALARS.saturating_add(1))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if boundaries.len() > MAX_COMPOUND_SCALARS {
            return false;
        }
        boundaries.push(word.len());

        if self.matches_simple_compound_with_triples(word, &boundaries, allow_keep_case, false) {
            return true;
        }
        self.compound.check_triple
            && self.compound.simplified_triple
            && self.matches_simplified_triple_compound(word, allow_keep_case)
            || self.matches_compound_pattern_replacements(word, allow_keep_case)
    }

    pub(crate) fn matches_simple_compound_with_triples(
        &self,
        word: &str,
        boundaries: &[usize],
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        self.compound.flag.as_ref().is_some_and(|flag| {
            self.matches_compound_pattern(
                word,
                boundaries,
                None,
                Some(flag),
                allow_keep_case,
                allow_boundary_triples,
            )
        }) || self.compound.rules.iter().any(|rule| {
            rule.patterns.iter().any(|pattern| {
                self.matches_compound_pattern(
                    word,
                    boundaries,
                    Some(pattern),
                    None,
                    allow_keep_case,
                    allow_boundary_triples,
                )
            })
        }) || self.matches_positioned_compound(
            word,
            boundaries,
            allow_keep_case,
            allow_boundary_triples,
        )
    }

    /// Matches either a generic `COMPOUNDFLAG` segmentation or one literal
    /// `COMPOUNDRULE` pattern over the bounded boundary set.
    ///
    /// `reachable[i]` means that the prefix ending at `boundaries[i]` can be
    /// formed by the flags consumed so far. Each transition only extends those
    /// reachable prefixes, so an unreachable suffix cannot become accepted by
    /// a later component.
    pub(crate) fn matches_compound_pattern(
        &self,
        word: &str,
        boundaries: &[usize],
        pattern: Option<&[Flag]>,
        generic_flag: Option<&Flag>,
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        if let Some(pattern) = pattern {
            return self.matches_fixed_compound_pattern(
                word,
                boundaries,
                pattern,
                allow_keep_case,
                allow_boundary_triples,
            );
        }
        let Some(flag) = generic_flag else {
            return false;
        };

        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        for component_count in 1..boundaries.len() {
            let next = self.extend_compound_components(
                word,
                boundaries,
                &reachable,
                *flag,
                allow_keep_case,
                allow_boundary_triples,
            );
            if component_count >= 2
                && next.last() == Some(&true)
                && self.compound_component_count_is_allowed(word, component_count)
            {
                return true;
            }
            if self.compound_component_count_cannot_continue(component_count) {
                return false;
            }
            reachable = next;
        }
        false
    }

    /// Matches one fixed `COMPOUNDRULE` flag sequence using the same bounded
    /// reachability representation as the generic compound path.
    pub(crate) fn matches_fixed_compound_pattern(
        &self,
        word: &str,
        boundaries: &[usize],
        pattern: &[Flag],
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        if pattern.len() < 2 {
            return false;
        }
        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        for flag in pattern {
            let next = self.extend_compound_components(
                word,
                boundaries,
                &reachable,
                *flag,
                allow_keep_case,
                allow_boundary_triples,
            );
            if next.iter().all(|reachable| !reachable) {
                return false;
            }
            reachable = next;
        }
        reachable.last() == Some(&true)
            && self.compound_component_count_is_allowed(word, pattern.len())
    }

    /// Adds one component transition to a compound reachability frontier.
    ///
    /// `minimum_length` counts Unicode scalar boundaries, not bytes. Therefore
    /// `first_end = start + minimum_length` skips every candidate that is too
    /// short while keeping slicing valid through the precomputed UTF-8 byte
    /// offsets in `boundaries`.
    pub(crate) fn extend_compound_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        flag: Flag,
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.compound_boundary_is_allowed(
                    word,
                    boundaries[start],
                    boundaries[end],
                    allow_boundary_triples,
                ) && self.matches_compound_component(
                    candidate,
                    flag,
                    end + 1 == boundaries.len(),
                    allow_keep_case,
                ) {
                    next[end] = true;
                }
            }
        }
        next
    }

    pub(crate) fn matches_compound_component(
        &self,
        word: &str,
        required_flag: Flag,
        is_final_component: bool,
        allow_keep_case: bool,
    ) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && (is_final_component || !self.is_compound_forbidden(&lexeme.flags))
                && has_flag(&lexeme.flags, required_flag)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
        })
    }

    /// Matches `COMPOUNDBEGIN`/`COMPOUNDMIDDLE`/`COMPOUNDEND` compounds.
    ///
    /// After the initial begin transition, `reachable[i]` means that a valid
    /// begin/middle parse covers `word[..boundaries[i]]`. Each loop iteration
    /// first tries an end component, so the `2..` range starts with the
    /// smallest valid two-component compound; only when it cannot terminate
    /// does it add one middle component for the next iteration. The same
    /// scalar-boundary `minimum_length` rule is applied by the transition
    /// helper.
    pub(crate) fn matches_positioned_compound(
        &self,
        word: &str,
        boundaries: &[usize],
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        let (Some(begin), Some(end)) = (&self.compound.begin, &self.compound.end) else {
            return false;
        };
        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        reachable = self.extend_positioned_components(
            word,
            boundaries,
            &reachable,
            *begin,
            CompoundPosition::Begin,
            allow_keep_case,
            allow_boundary_triples,
        );
        for component_count in 2..boundaries.len() {
            let terminal = self.extend_positioned_components(
                word,
                boundaries,
                &reachable,
                *end,
                CompoundPosition::End,
                allow_keep_case,
                allow_boundary_triples,
            );
            if terminal.last() == Some(&true)
                && self.compound_component_count_is_allowed(word, component_count)
            {
                return true;
            }
            if self.compound_component_count_cannot_continue(component_count) {
                return false;
            }
            let Some(middle) = self.compound.middle.as_ref() else {
                return false;
            };
            reachable = self.extend_positioned_components(
                word,
                boundaries,
                &reachable,
                *middle,
                CompoundPosition::Middle,
                allow_keep_case,
                allow_boundary_triples,
            );
        }
        false
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "compound position, casing, and triple policy describe one bounded transition"
    )]
    /// Extends one positioned-compound DP frontier by a single component.
    ///
    /// `next[end]` is true exactly when some `reachable[start]` prefix can
    /// append an eligible component from `start` to `end` at the requested
    /// position. The returned vector is a new frontier so a middle transition
    /// cannot mutate the set being used for the current end attempt.
    pub(crate) fn extend_positioned_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        position_flag: Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.compound_boundary_is_allowed(
                    word,
                    boundaries[start],
                    boundaries[end],
                    allow_boundary_triples,
                ) && self.matches_positioned_component(
                    candidate,
                    position_flag,
                    position,
                    allow_keep_case,
                ) {
                    next[end] = true;
                }
            }
        }
        next
    }

    pub(crate) fn matches_positioned_component(
        &self,
        word: &str,
        position_flag: Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && (position == CompoundPosition::End || !self.is_compound_forbidden(&lexeme.flags))
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                && (has_flag(&lexeme.flags, position_flag)
                    || self
                        .compound
                        .flag
                        .as_ref()
                        .is_some_and(|flag| has_flag(&lexeme.flags, *flag)))
        }) || self.matches_one_affix_compound_component(
            word,
            position_flag,
            position,
            allow_keep_case,
        )
    }

    /// Resolves one `COMPOUNDPERMITFLAG` affix inside a positioned component.
    ///
    /// Permit-affix matching is intentionally one inverse rule application;
    /// multi-step permit chains remain outside the documented compatibility
    /// subset. Position checks keep prefixes at the beginning, suffixes at the
    /// end, and reject both in the middle.
    pub(crate) fn matches_one_affix_compound_component(
        &self,
        word: &str,
        position_flag: Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> bool {
        self.candidate_affix_rules(word)
            .filter(|rule| self.compound_rule_is_allowed(rule, position))
            .any(|rule| {
                rule.reverse_apply(word, self.full_strip)
                    .is_some_and(|stem| {
                        self.lexemes_for_stem(&stem).any(|lexeme| {
                            !self.is_forbidden(&lexeme.flags)
                                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                                && has_flag(&lexeme.flags, rule.flag)
                                && (has_flag(&lexeme.flags, position_flag)
                                    || self
                                        .compound
                                        .flag
                                        .as_ref()
                                        .is_some_and(|flag| has_flag(&lexeme.flags, *flag)))
                                && self.is_accepted_compound_state(&FormState::new(lexeme).apply(
                                    rule,
                                    word.to_owned(),
                                    &self.special_flags,
                                ))
                        })
                    })
            })
    }

    pub(crate) fn compound_component_count_is_allowed(
        &self,
        word: &str,
        component_count: usize,
    ) -> bool {
        let Some(maximum_words) = self.compound.maximum_words else {
            return true;
        };
        if component_count <= maximum_words {
            return true;
        }
        self.compound.syllable_limit.as_ref().is_some_and(|limit| {
            word.chars()
                .filter(|character| limit.vowels.contains(character))
                .take(limit.maximum.saturating_add(1))
                .count()
                <= limit.maximum
        })
    }

    pub(crate) fn compound_component_count_cannot_continue(&self, component_count: usize) -> bool {
        self.compound.maximum_words.is_some_and(|maximum_words| {
            component_count >= maximum_words && self.compound.syllable_limit.is_none()
        })
    }

    pub(crate) fn compound_boundary_is_allowed(
        &self,
        word: &str,
        start: usize,
        end: usize,
        allow_boundary_triples: bool,
    ) -> bool {
        let component = &word[start..end];
        if self.compound.check_case
            && start != 0
            && component.chars().next().is_some_and(char::is_uppercase)
        {
            return false;
        }
        if self.compound.check_duplicate
            && start >= component.len()
            && word[..start].ends_with(component)
        {
            return false;
        }
        if self.compound.check_triple
            && !allow_boundary_triples
            && has_triple_at_compound_boundary(word, start)
        {
            return false;
        }
        if end == word.len()
            && self.compound.force_uppercase.as_ref().is_some_and(|flag| {
                self.lexemes_for_stem(component)
                    .any(|lexeme| has_flag(&lexeme.flags, *flag))
            })
            && !word.chars().next().is_some_and(char::is_uppercase)
        {
            return false;
        }
        !self.compound_pattern_forbids(word, start, component)
    }

    pub(crate) fn matches_simplified_triple_compound(
        &self,
        word: &str,
        allow_keep_case: bool,
    ) -> bool {
        for (boundary, character) in word.char_indices().skip(1) {
            let Some(previous) = word[..boundary].chars().last() else {
                continue;
            };
            if character != previous {
                continue;
            }
            let mut expanded = String::with_capacity(word.len() + previous.len_utf8());
            expanded.push_str(&word[..boundary]);
            expanded.push(previous);
            expanded.push_str(&word[boundary..]);
            let Some(boundaries) = compound_boundaries(&expanded) else {
                continue;
            };
            if self.matches_simple_compound_with_triples(
                &expanded,
                &boundaries,
                allow_keep_case,
                true,
            ) {
                return true;
            }
        }
        false
    }

    pub(crate) fn matches_compound_pattern_replacements(
        &self,
        word: &str,
        allow_keep_case: bool,
    ) -> bool {
        self.compound.patterns.iter().any(|pattern| {
            let Some(replacement) = pattern.replacement.as_deref() else {
                return false;
            };
            word.match_indices(replacement)
                .take(MAX_COMPOUND_PATTERN_REPLACEMENT_VARIANTS)
                .any(|(start, _)| {
                    let end = start + replacement.len();
                    let mut expanded = String::with_capacity(
                        word.len()
                            .saturating_add(pattern.ending.len())
                            .saturating_add(pattern.beginning.len())
                            .saturating_sub(replacement.len()),
                    );
                    expanded.push_str(&word[..start]);
                    expanded.push_str(&pattern.ending);
                    expanded.push_str(&pattern.beginning);
                    expanded.push_str(&word[end..]);
                    compound_boundaries(&expanded).is_some_and(|boundaries| {
                        self.matches_simple_compound_with_triples(
                            &expanded,
                            &boundaries,
                            allow_keep_case,
                            false,
                        )
                    })
                })
        })
    }

    /// Applies `CHECKCOMPOUNDPATTERN` to one candidate component boundary.
    ///
    /// The left and right strings are checked against the declared ending and
    /// beginning patterns, with optional flags validated against the selected
    /// components. This is a boundary guard, not a recursive compound match.
    pub(crate) fn compound_pattern_forbids(&self, word: &str, start: usize, right: &str) -> bool {
        self.compound.patterns.iter().any(|pattern| {
            pattern.replacement.is_none()
                && pattern.ending.as_ref() != "0"
                && word[..start].ends_with(pattern.ending.as_ref())
                && right.starts_with(pattern.beginning.as_ref())
                && pattern.ending_flag.as_ref().is_none_or(|flag| {
                    self.lexemes_for_stem(&word[..start])
                        .any(|lexeme| has_flag(&lexeme.flags, *flag))
                })
                && pattern.beginning_flag.as_ref().is_none_or(|flag| {
                    self.lexemes_for_stem(right)
                        .any(|lexeme| has_flag(&lexeme.flags, *flag))
                })
        })
    }

    pub(crate) fn matches_noncompound_replacement(
        &self,
        word: &str,
        allow_keep_case: bool,
    ) -> bool {
        self.replacement_rules.iter().any(|rule| {
            word.match_indices(rule.from()).any(|(start, _)| {
                let end = start + rule.from().len();
                if (rule.at_word_start() && start != 0) || (rule.at_word_end() && end != word.len())
                {
                    return false;
                }
                let mut corrected = String::with_capacity(
                    word.len()
                        .saturating_add(rule.to().len())
                        .saturating_sub(rule.from().len()),
                );
                corrected.push_str(&word[..start]);
                corrected.push_str(rule.to());
                corrected.push_str(&word[end..]);
                self.matches_noncompound_word(&corrected, allow_keep_case)
            })
        })
    }

    pub(crate) fn matches_noncompound_word(&self, word: &str, allow_keep_case: bool) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && !self.requires_affix(&lexeme.flags)
                && !self.is_only_in_compound(&lexeme.flags)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
        }) || self.matches_single_affix_word(word, allow_keep_case)
    }

    pub(crate) fn matches_break_word(&self, word: &str, allow_keep_case: bool) -> bool {
        if self.break_patterns.is_empty() || word.chars().count() > MAX_COMPOUND_SCALARS {
            return false;
        }
        self.break_patterns.iter().any(|pattern| {
            if pattern.at_start {
                return word
                    .strip_prefix(pattern.text.as_ref())
                    .is_some_and(|rest| {
                        !rest.is_empty() && self.matches_without_break(rest, allow_keep_case)
                    });
            }
            if pattern.at_end {
                return word
                    .strip_suffix(pattern.text.as_ref())
                    .is_some_and(|rest| {
                        !rest.is_empty() && self.matches_without_break(rest, allow_keep_case)
                    });
            }
            word.match_indices(pattern.text.as_ref()).any(|(start, _)| {
                let end = start + pattern.text.len();
                let (left, right) = (&word[..start], &word[end..]);
                !left.is_empty()
                    && !right.is_empty()
                    && self.matches_without_break(left, allow_keep_case)
                    && self.matches_without_break(right, allow_keep_case)
            })
        })
    }
}

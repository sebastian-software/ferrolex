use std::collections::{BTreeMap, BTreeSet};

/// A lazily-built, source-neutral index for bounded spelling candidates.
///
/// The index keeps candidates in their source order and uses lowercase scalar
/// lengths plus character postings to discard words that cannot be within a
/// requested edit distance. The character-overlap bound is conservative for
/// insertions, deletions, substitutions, and adjacent transpositions, so it
/// never removes a candidate accepted by an OSA-distance check.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CandidateIndex {
    candidates: Vec<Box<str>>,
    lowercase_characters: Vec<Option<Box<[char]>>>,
    indices_by_length: BTreeMap<usize, Vec<usize>>,
    indices_by_character: BTreeMap<char, Vec<usize>>,
    indexed_maximum_word_scalars: usize,
}

impl CandidateIndex {
    /// Builds an index from candidates already arranged in deterministic order.
    #[must_use]
    pub fn new<'candidate>(
        candidates: impl IntoIterator<Item = &'candidate str>,
        maximum_word_scalars: usize,
    ) -> Self {
        let mut index = Self {
            indexed_maximum_word_scalars: maximum_word_scalars,
            ..Self::default()
        };
        for candidate in candidates {
            let candidate_index = index.candidates.len();
            index.candidates.push(Box::from(candidate));
            let Some(mut characters) = lowercase_bounded(candidate, maximum_word_scalars) else {
                index.lowercase_characters.push(None);
                continue;
            };
            index
                .indices_by_length
                .entry(characters.len())
                .or_default()
                .push(candidate_index);
            characters.sort_unstable();
            for character in characters.iter().copied() {
                let postings = index.indices_by_character.entry(character).or_default();
                if postings.last() != Some(&candidate_index) {
                    postings.push(candidate_index);
                }
            }
            index.lowercase_characters.push(Some(characters.into()));
        }
        index
    }

    /// Visits candidates that can still be within `maximum_distance` of `query`.
    ///
    /// Candidates are emitted in their original deterministic order. Returning
    /// `false` from `visitor` stops the traversal.
    pub fn visit_nearby(
        &self,
        query: &[char],
        maximum_distance: usize,
        maximum_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        if maximum_word_scalars > self.indexed_maximum_word_scalars {
            self.visit_nearby_linear(query, maximum_distance, maximum_word_scalars, visitor);
            return;
        }
        let minimum_length = query.len().saturating_sub(maximum_distance);
        let maximum_length = query
            .len()
            .saturating_add(maximum_distance)
            .min(maximum_word_scalars);
        let mut possible = BTreeSet::new();

        if query.is_empty() || maximum_distance >= query.len() {
            for (_, indices) in self
                .indices_by_length
                .range(minimum_length..=maximum_length)
            {
                possible.extend(indices.iter().copied());
            }
        } else {
            for character in query.iter().copied().collect::<BTreeSet<_>>() {
                if let Some(indices) = self.indices_by_character.get(&character) {
                    possible.extend(indices.iter().copied());
                }
            }
        }

        let mut sorted_query = query.to_vec();
        sorted_query.sort_unstable();
        for candidate_index in possible {
            let Some(candidate_characters) = &self.lowercase_characters[candidate_index] else {
                continue;
            };
            if candidate_characters.len() < minimum_length
                || candidate_characters.len() > maximum_length
            {
                continue;
            }
            let minimum_overlap = query
                .len()
                .max(candidate_characters.len())
                .saturating_sub(maximum_distance);
            if multiset_overlap(&sorted_query, candidate_characters) < minimum_overlap {
                continue;
            }
            if !visitor(&self.candidates[candidate_index]) {
                break;
            }
        }
    }

    fn visit_nearby_linear(
        &self,
        query: &[char],
        maximum_distance: usize,
        maximum_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        let mut sorted_query = query.to_vec();
        sorted_query.sort_unstable();
        let minimum_length = query.len().saturating_sub(maximum_distance);
        let maximum_length = query
            .len()
            .saturating_add(maximum_distance)
            .min(maximum_word_scalars);
        for candidate in &self.candidates {
            let Some(mut candidate_characters) = lowercase_bounded(candidate, maximum_word_scalars)
            else {
                continue;
            };
            if candidate_characters.len() < minimum_length
                || candidate_characters.len() > maximum_length
            {
                continue;
            }
            candidate_characters.sort_unstable();
            let minimum_overlap = query
                .len()
                .max(candidate_characters.len())
                .saturating_sub(maximum_distance);
            if multiset_overlap(&sorted_query, &candidate_characters) < minimum_overlap {
                continue;
            }
            if !visitor(candidate) {
                break;
            }
        }
    }
}

fn lowercase_bounded(word: &str, maximum: usize) -> Option<Vec<char>> {
    let mut lowercase = Vec::new();
    for character in word.chars() {
        for lowercase_character in character.to_lowercase() {
            if lowercase.len() == maximum {
                return None;
            }
            lowercase.push(lowercase_character);
        }
    }
    Some(lowercase)
}

fn multiset_overlap(left: &[char], right: &[char]) -> usize {
    let (mut left_index, mut right_index, mut overlap) = (0, 0, 0);
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                overlap += 1;
                left_index += 1;
                right_index += 1;
            }
        }
    }
    overlap
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::CandidateIndex;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn filtering_retains_every_bounded_osa_neighbor(
            candidates in proptest::collection::vec("[a-z]{1,8}", 1..32),
            query in "[a-z]{0,8}",
            maximum_distance in 0_usize..=3,
        ) {
            let index = CandidateIndex::new(candidates.iter().map(String::as_str), 8);
            let query_characters = query.chars().collect::<Vec<_>>();
            let mut visited = BTreeSet::new();
            index.visit_nearby(&query_characters, maximum_distance, 8, &mut |candidate| {
                visited.insert(candidate.to_owned());
                true
            });

            for candidate in &candidates {
                if osa_distance(&query, candidate) <= maximum_distance {
                    prop_assert!(visited.contains(candidate), "filtered valid candidate {candidate:?} for query {query:?}");
                }
            }
        }
    }

    #[test]
    fn retains_transpositions_and_late_lexical_neighbors() {
        let index = CandidateIndex::new(["aaaaaaa", "receive", "zzzzzzz"], 64);
        let query = "recieve".chars().collect::<Vec<_>>();
        let mut candidates = Vec::new();

        index.visit_nearby(&query, 1, 64, &mut |candidate| {
            candidates.push(candidate.to_owned());
            true
        });

        assert_eq!(candidates, ["receive"]);
    }

    #[test]
    fn preserves_duplicates_in_character_overlap_counts() {
        let index = CandidateIndex::new(["book", "back", "cook"], 64);
        let query = "boook".chars().collect::<Vec<_>>();
        let mut candidates = Vec::new();

        index.visit_nearby(&query, 1, 64, &mut |candidate| {
            candidates.push(candidate.to_owned());
            true
        });

        assert_eq!(candidates, ["book"]);
    }

    #[test]
    fn a_later_larger_limit_falls_back_without_losing_candidates() {
        let index = CandidateIndex::new(["short", "tiny"], 4);
        let query = "shrot".chars().collect::<Vec<_>>();
        let mut candidates = Vec::new();

        index.visit_nearby(&query, 1, 5, &mut |candidate| {
            candidates.push(candidate.to_owned());
            true
        });

        assert_eq!(candidates, ["short"]);
    }

    fn osa_distance(left: &str, right: &str) -> usize {
        let left = left.chars().collect::<Vec<_>>();
        let right = right.chars().collect::<Vec<_>>();
        let mut previous_previous = vec![0; right.len() + 1];
        let mut previous = (0..=right.len()).collect::<Vec<_>>();
        for (left_index, left_character) in left.iter().enumerate() {
            let mut current = vec![left_index + 1; right.len() + 1];
            for (right_index, right_character) in right.iter().enumerate() {
                let cost = usize::from(left_character != right_character);
                current[right_index + 1] = (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + cost);
                if left_index > 0
                    && right_index > 0
                    && *left_character == right[right_index - 1]
                    && left[left_index - 1] == *right_character
                {
                    current[right_index + 1] =
                        current[right_index + 1].min(previous_previous[right_index - 1] + 1);
                }
            }
            previous_previous = previous;
            previous = current;
        }
        previous[right.len()]
    }
}

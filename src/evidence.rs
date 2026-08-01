//! Shared Unicode evidence primitives for alignment and set-aware ranking.

use std::collections::HashSet;

use unicode_categories::UnicodeCategories;
use unicode_normalization::UnicodeNormalization;

use crate::SearchResult;

const MAX_RESULT_EVIDENCE_CHARS: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryUnit {
    pub(crate) source: String,
    pub(crate) normalized: Vec<char>,
    pub(crate) requires_exact: bool,
}

pub(crate) fn query_units_with_source(value: &str) -> Vec<QueryUnit> {
    let mut seen = HashSet::new();
    let mut units = Vec::new();
    let mut source = String::new();

    let mut finish_unit = |source: &mut String| {
        if source.is_empty() {
            return;
        }
        let normalized = normalized_characters(source);
        let requires_exact = source.chars().any(is_intra_unit_connector);
        if normalized.is_empty() {
            source.clear();
        } else if seen.insert(normalized.clone()) {
            units.push(QueryUnit {
                source: std::mem::take(source),
                normalized,
                requires_exact,
            });
        } else {
            if requires_exact {
                if let Some(unit) = units.iter_mut().find(|unit| unit.normalized == normalized) {
                    unit.requires_exact = true;
                }
            }
            source.clear();
        }
    };

    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        let connected = !source.is_empty()
            && is_intra_unit_connector(character)
            && characters
                .peek()
                .is_some_and(|next| is_query_unit_character(*next));
        if is_query_unit_character(character) || connected {
            source.push(character);
        } else {
            finish_unit(&mut source);
        }
    }
    finish_unit(&mut source);
    units
}

fn is_query_unit_character(character: char) -> bool {
    character.is_alphanumeric() || character.is_mark()
}

fn is_intra_unit_connector(character: char) -> bool {
    matches!(
        character,
        '-' | '\u{2010}'
            | '\u{2011}'
            | '\u{2012}'
            | '\u{2013}'
            | '\u{2014}'
            | '\u{2015}'
            | '\u{2212}'
            | '/'
            | ':'
            | '.'
            | '_'
            | '\''
            | '\u{2019}'
            | '@'
    )
}

pub(crate) fn normalized_query_units(value: &str) -> Vec<Vec<char>> {
    query_units_with_source(value)
        .into_iter()
        .map(|unit| unit.normalized)
        .collect()
}

pub(crate) fn normalized_characters(value: &str) -> Vec<char> {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

pub(crate) fn contains_characters(haystack: &[char], needle: &[char]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

pub(crate) fn character_grams(value: &[char], size: usize) -> HashSet<Vec<char>> {
    if size == 0 || value.len() < size {
        return HashSet::new();
    }
    value.windows(size).map(<[char]>::to_vec).collect()
}

pub(crate) fn query_unit_is_represented_with_exactness(
    unit: &[char],
    visible: &[Vec<char>],
    requires_exact: bool,
) -> bool {
    if unit.is_empty() || visible.is_empty() {
        return false;
    }
    if visible.iter().any(|field| contains_characters(field, unit)) {
        return true;
    }
    if requires_exact {
        return false;
    }

    let gram_size = unit.len().min(3);
    let query_grams = character_grams(unit, gram_size);
    let visible_grams = visible
        .iter()
        .flat_map(|field| character_grams(field, gram_size))
        .collect::<HashSet<_>>();
    let matched = query_grams
        .iter()
        .filter(|gram| visible_grams.contains(*gram))
        .count();
    !query_grams.is_empty() && matched.saturating_mul(2) >= query_grams.len()
}

/// Normalizes caller-visible result evidence under one per-result work bound.
///
/// Title and snippet evidence are retained first because they remain the
/// authoritative fields for base query alignment. Full text can supplement
/// set coverage, but cannot force unbounded normalization work.
pub(crate) fn normalized_result_evidence_fields(result: &SearchResult) -> Vec<Vec<char>> {
    let mut remaining = MAX_RESULT_EVIDENCE_CHARS;
    let mut fields = Vec::with_capacity(3);

    for value in [
        Some(result.title.as_str()),
        Some(result.content.as_str()),
        result.full_text.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if remaining == 0 {
            break;
        }
        let normalized = value
            .nfkc()
            .flat_map(char::to_lowercase)
            .filter(|character| character.is_alphanumeric())
            .take(remaining)
            .collect::<Vec<_>>();
        remaining = remaining.saturating_sub(normalized.len());
        if !normalized.is_empty() {
            fields.push(normalized);
        }
    }

    fields
}

pub(crate) fn normalized_full_text_evidence(result: &SearchResult) -> Option<Vec<char>> {
    result.full_text.as_deref().and_then(|full_text| {
        let normalized = full_text
            .nfkc()
            .flat_map(char::to_lowercase)
            .filter(|character| character.is_alphanumeric())
            .take(MAX_RESULT_EVIDENCE_CHARS)
            .collect::<Vec<_>>();
        (!normalized.is_empty()).then_some(normalized)
    })
}

/// Query evidence atoms shared by the greedy set-coverage ranker.
///
/// Whitespace- or punctuation-delimited queries use their normalized units.
/// A single unsegmented unit uses unique character trigrams so the same
/// mechanism remains useful for scripts that commonly omit spaces.
pub(crate) struct QueryEvidence {
    atoms: Vec<QueryEvidenceAtom>,
    total_weight: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryEvidenceAtom {
    value: Vec<char>,
    requires_exact: bool,
}

impl QueryEvidence {
    pub(crate) fn new(query: &str) -> Self {
        let units = query_units_with_source(query);
        let mut atoms = if units.len() > 1 || units.first().is_some_and(|unit| unit.requires_exact)
        {
            units
                .into_iter()
                .map(|unit| QueryEvidenceAtom {
                    value: unit.normalized,
                    requires_exact: unit.requires_exact,
                })
                .collect()
        } else {
            let characters = units
                .into_iter()
                .next()
                .map(|unit| unit.normalized)
                .unwrap_or_default();
            let gram_size = characters.len().min(3);
            if gram_size == 0 {
                Vec::new()
            } else {
                let mut grams = character_grams(&characters, gram_size)
                    .into_iter()
                    .collect::<Vec<_>>();
                grams.sort_unstable();
                grams
                    .into_iter()
                    .map(|value| QueryEvidenceAtom {
                        value,
                        requires_exact: false,
                    })
                    .collect()
            }
        };
        atoms.dedup();
        let total_weight = atoms.iter().map(|atom| atom.value.len()).sum();
        Self {
            atoms,
            total_weight,
        }
    }

    pub(crate) fn is_composite(&self) -> bool {
        self.atoms.len() > 1 && self.total_weight > 0
    }

    pub(crate) fn matching_atoms(&self, result: &SearchResult) -> Vec<usize> {
        let visible = normalized_result_evidence_fields(result);
        self.atoms
            .iter()
            .enumerate()
            .filter_map(|(index, atom)| {
                query_unit_is_represented_with_exactness(&atom.value, &visible, atom.requires_exact)
                    .then_some(index)
            })
            .collect()
    }

    pub(crate) fn marginal_coverage(&self, matching: &[usize], covered: &HashSet<usize>) -> f64 {
        if self.total_weight == 0 {
            return 0.0;
        }
        let new_weight = matching
            .iter()
            .filter(|index| !covered.contains(index))
            .map(|index| self.atoms[*index].value.len())
            .sum::<usize>();
        new_weight as f64 / self.total_weight as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_evidence_normalization_is_bounded() {
        let mut result = SearchResult::new("https://example.test", "", "");
        result.full_text = Some(format!(
            "{}terminalneedle",
            "a".repeat(MAX_RESULT_EVIDENCE_CHARS)
        ));

        let fields = normalized_result_evidence_fields(&result);
        let normalized_length = fields.iter().map(Vec::len).sum::<usize>();

        assert_eq!(normalized_length, MAX_RESULT_EVIDENCE_CHARS);
        assert!(!fields
            .iter()
            .any(|field| contains_characters(field, &normalized_characters("terminalneedle"))));
    }

    #[test]
    fn query_units_preserve_marked_words_and_unsegmented_ideographs() {
        let devanagari = normalized_query_units("सौर कृषि पंप");
        assert_eq!(devanagari.len(), 3);
        assert_eq!(devanagari[0], normalized_characters("सौर"));
        assert_eq!(devanagari[1], normalized_characters("कृषि"));
        assert_eq!(devanagari[2], normalized_characters("पंप"));

        assert_eq!(
            normalized_query_units("跨境交通运行表现报告"),
            vec![normalized_characters("跨境交通运行表现报告")]
        );
    }

    #[test]
    fn query_units_preserve_connected_identifiers_as_single_atoms() {
        assert_eq!(
            normalized_query_units("HTTP/3 RFC 9114"),
            vec![
                normalized_characters("HTTP/3"),
                normalized_characters("RFC"),
                normalized_characters("9114"),
            ]
        );
        assert_eq!(
            normalized_query_units("ISO 14068-1:2023"),
            vec![
                normalized_characters("ISO"),
                normalized_characters("14068-1:2023"),
            ]
        );
    }
}

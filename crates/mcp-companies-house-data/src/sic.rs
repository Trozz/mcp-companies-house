//! The SIC 2007 codes Companies House accepts (its condensed list), with a word search.
//!
//! `data/sic2007.tsv` is Companies House's "SIC 2007 condensed list" (published on gov.uk).
//! Contains public sector information licensed under the Open Government Licence v3.0.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const DATA: &str = include_str!("../data/sic2007.tsv");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SicCode {
    pub code: &'static str,
    pub description: &'static str,
    /// SIC 2007 section letter, A to U.
    pub section: &'static str,
}

static CODES: LazyLock<Vec<SicCode>> = LazyLock::new(|| {
    DATA.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split('\t');
            Some(SicCode {
                code: cols.next()?,
                description: cols.next()?,
                section: cols.next()?,
            })
        })
        .collect()
});

static BY_CODE: LazyLock<HashMap<&'static str, &'static SicCode>> =
    LazyLock::new(|| CODES.iter().map(|c| (c.code, c)).collect());

/// Every word of every description, hyphenated words both whole and split.
static DESCRIPTION_WORDS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    CODES
        .iter()
        .flat_map(|c| words(c.description, true))
        .collect()
});

/// Everyday words that do not appear in the official descriptions, mapped to words that do.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("bakery", &["bread"]),
    ("bakeries", &["bread"]),
    ("baker", &["bread"]),
    ("bakers", &["bread"]),
    ("dentist", &["dental"]),
    ("dentists", &["dental"]),
    ("vet", &["veterinary"]),
    ("vets", &["veterinary"]),
    ("garage", &["motor"]),
    ("mechanic", &["motor"]),
    ("marketing", &["advertising", "market"]),
    ("builder", &["construction", "building"]),
    ("builders", &["construction", "building"]),
    ("lawyer", &["legal", "solicitors", "barristers"]),
    ("lawyers", &["legal", "solicitors", "barristers"]),
    ("accountant", &["accounting", "bookkeeping"]),
    ("accountants", &["accounting", "bookkeeping"]),
    ("doctor", &["medical"]),
    ("doctors", &["medical"]),
    ("gp", &["medical"]),
    ("pharmacy", &["pharmaceutical", "chemist"]),
    ("gym", &["fitness"]),
    ("nursery", &["day-care", "pre-primary"]),
    ("pre-school", &["pre-primary"]),
    ("preschool", &["pre-primary"]),
    ("landlord", &["letting", "renting"]),
    ("it", &["information", "computer"]),
    ("tech", &["information", "computer", "software"]),
    ("e-commerce", &["internet"]),
    ("ecommerce", &["internet"]),
    ("online", &["internet"]),
];

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "for", "in", "of", "on", "or", "the", "to", "with",
];

pub fn all() -> &'static [SicCode] {
    &CODES
}

/// The official description of a code, if Companies House lists it.
pub fn describe(code: &str) -> Option<&'static str> {
    BY_CODE.get(code).map(|c| c.description)
}

/// Lower-case words of `text`, hyphenated words kept whole. With `split_hyphens`, the parts of a
/// hyphenated word are added too (for descriptions, so "day care" finds "day-care"). Single
/// letters are dropped: they come from abbreviations such as "n.e.c." and match nothing useful.
fn words(text: &str, split_hyphens: bool) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .flat_map(|w| {
            let w = w.trim_matches('-').to_ascii_lowercase();
            let mut out = vec![w.clone()];
            if split_hyphens && w.contains('-') {
                out.extend(w.split('-').map(str::to_string));
            }
            out
        })
        .filter(|w| w.len() > 1)
        .collect()
}

/// The prefix a query word must start a description word with: short words match whole, longer
/// ones by their first 4 or 5 letters, so "accountant" finds "accounting" and "agent" finds
/// "agencies".
fn stem(word: &str) -> &str {
    match word.len() {
        0..=4 => word,
        5 => &word[..4],
        _ => &word[..5],
    }
}

/// The words to look for. A hyphenated query word is kept whole when it has a synonym, appears
/// whole in a description ("day-care") or has a one-letter part ("e-commerce", "t-shirt").
/// Otherwise it is joined when that starts a description word ("re-insurance" finds
/// "reinsurance"), or else split ("real-estate" looks for "real" and "estate").
fn query_words(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in words(q, false) {
        let parts: Vec<&str> = w.split('-').collect();
        let keep_whole = parts.len() == 1
            || parts.iter().any(|p| p.len() < 2)
            || SYNONYMS.iter().any(|(k, _)| *k == w)
            || DESCRIPTION_WORDS.contains(&w);
        if keep_whole {
            out.push(w);
            continue;
        }
        let joined = parts.concat();
        if DESCRIPTION_WORDS.iter().any(|d| d.starts_with(&joined)) {
            out.push(joined);
        } else {
            out.extend(parts.into_iter().map(str::to_string));
        }
    }
    out.retain(|w| !STOP_WORDS.contains(&w.as_str()));
    out
}

fn word_matches(query_word: &str, description_words: &[String]) -> bool {
    let short = query_word.len() <= 3;
    let s = stem(query_word);
    description_words.iter().any(|d| {
        if short {
            d == query_word
        } else {
            d.starts_with(s)
        }
    })
}

/// Codes whose description matches `query`, best first. A query of digits matches codes starting
/// with them. Otherwise every word must match a description word (or one of its synonyms); when
/// no code matches every word, codes matching the most words are returned.
pub fn search(query: &str, limit: usize) -> Vec<&'static SicCode> {
    let q = query.trim();
    if !q.is_empty() && q.chars().all(|c| c.is_ascii_digit()) {
        return CODES
            .iter()
            .filter(|c| c.code.starts_with(q))
            .take(limit)
            .collect();
    }
    let query_words = query_words(q);
    if query_words.is_empty() {
        return Vec::new();
    }
    let phrase = q.to_ascii_lowercase();
    // (words matched, words matched whole, has the whole phrase, code)
    let mut scored: Vec<(usize, usize, bool, &'static SicCode)> = CODES
        .iter()
        .filter_map(|c| {
            let dw = words(c.description, true);
            let mut matched = 0;
            let mut whole = 0;
            for w in &query_words {
                // Synonym targets are official words: they must start a description word.
                let alternatives = SYNONYMS
                    .iter()
                    .find(|(k, _)| *k == w.as_str())
                    .map(|(_, v)| *v)
                    .unwrap_or_default();
                let by_synonym = alternatives
                    .iter()
                    .any(|a| dw.iter().any(|d| d.starts_with(a)));
                // Short words only ever match whole ("tax" is not "taxi").
                let short = w.len() <= 3;
                let exact = dw.iter().any(|d| {
                    if short {
                        d == w
                    } else {
                        d.starts_with(w.as_str())
                    }
                });
                if exact || by_synonym {
                    matched += 1;
                    whole += 1;
                } else if word_matches(w, &dw) {
                    matched += 1;
                }
            }
            (matched > 0).then(|| {
                let has_phrase = c.description.to_ascii_lowercase().contains(&phrase);
                (matched, whole, has_phrase, c)
            })
        })
        .collect();
    let best = scored.iter().map(|s| s.0).max().unwrap_or(0);
    if best == query_words.len() {
        scored.retain(|s| s.0 == best);
    }
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(b.2.cmp(&a.2))
            .then(a.3.code.cmp(b.3.code))
    });
    scored.into_iter().take(limit).map(|s| s.3).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(query: &str) -> Vec<&'static str> {
        search(query, 50).iter().map(|c| c.code).collect()
    }

    #[test]
    fn the_dataset_is_complete_and_well_formed() {
        assert_eq!(all().len(), 731);
        let mut seen = std::collections::HashSet::new();
        for c in all() {
            assert!(
                c.code.len() == 5 && c.code.chars().all(|ch| ch.is_ascii_digit()),
                "{c:?}"
            );
            assert!(!c.description.is_empty(), "{c:?}");
            assert!(
                c.section.len() == 1 && ("A"..="U").contains(&c.section),
                "{c:?}"
            );
            assert!(seen.insert(c.code), "duplicate {}", c.code);
        }
    }

    #[test]
    fn known_codes_are_described() {
        assert_eq!(
            describe("62012"),
            Some("Business and domestic software development")
        );
        assert_eq!(describe("99999"), Some("Dormant Company"));
        assert_eq!(describe("74990"), Some("Non-trading company"));
        assert_eq!(describe("00000"), None);
    }

    #[test]
    fn digits_match_code_prefixes() {
        let c = codes("6201");
        assert_eq!(c, vec!["62011", "62012"]);
        assert!(codes("62").iter().all(|c| c.starts_with("62")));
    }

    #[test]
    fn words_match_by_stem_and_synonym() {
        let bakery = codes("bakery");
        assert!(
            bakery.contains(&"10710") && bakery.contains(&"47240"),
            "{bakery:?}"
        );
        assert!(codes("software").contains(&"62012"));
        assert!(codes("accountant").contains(&"69201"));
        assert_eq!(codes("estate agent"), vec!["68310"]);
        assert!(codes("dentist").contains(&"86230"));
        assert!(
            !codes("dentist").contains(&"52219"),
            "incidental is not dental"
        );
        assert!(codes("cafe").contains(&"56102"));
        assert!(codes("hairdresser").contains(&"96020"));
        assert!(codes("plumber").contains(&"43220"));
    }

    #[test]
    fn all_words_must_match_when_possible() {
        let c = codes("software development");
        assert!(c.contains(&"62012"), "{c:?}");
        assert!(
            !c.contains(&"58290"),
            "publishing does not mention development: {c:?}"
        );
    }

    #[test]
    fn partial_matches_are_a_fallback() {
        let c = codes("software zebra");
        assert!(c.contains(&"62012"), "{c:?}");
    }

    #[test]
    fn hyphenated_queries_do_not_match_abbreviation_letters() {
        assert_eq!(codes("e-commerce"), vec!["47910"]);
        let retail = codes("e-commerce retail");
        assert!(
            retail
                .iter()
                .all(|c| describe(c).is_some_and(|d| !d.ends_with("n.e.c."))),
            "{retail:?}"
        );
        assert!(codes("day care").contains(&"88910"));
        assert!(codes("day-care").contains(&"88910"));
    }

    #[test]
    fn hyphenated_queries_match_words_written_apart_or_joined() {
        let estate = codes("real-estate");
        assert!(estate.contains(&"68310"), "{estate:?}");
        assert_eq!(estate, codes("real estate"));
        assert_eq!(codes("mail-order")[0], "47910");
        assert_eq!(codes("taxi-cab")[0], "49320");
        assert_eq!(codes("dry-cleaning")[0], "96010");
        assert!(codes("hair-dressing").contains(&"96020"));
        assert!(codes("re-insurance").contains(&"65201"));
        assert!(codes("self-storage").contains(&"52103"));
        assert_eq!(codes("pre-school"), vec!["85100"]);
    }

    #[test]
    fn short_words_match_only_whole_words() {
        assert_eq!(codes("tax"), vec!["69203"]);
        assert!(
            !codes("pet").iter().any(|c| c.starts_with("06")),
            "petroleum"
        );
        assert!(
            !codes("pub").iter().any(|c| c.starts_with("58")),
            "publishing"
        );
        assert_eq!(codes("art")[0], "47781");
        let repair = search("car repair", 10)
            .iter()
            .map(|c| c.code)
            .collect::<Vec<_>>();
        assert!(repair.contains(&"45200"), "{repair:?}");
    }

    #[test]
    fn whole_word_matches_rank_above_stem_only_ones() {
        // "internet" before "intermediate"; "clothing" before "clothes".
        assert_eq!(codes("internet")[0], "47910");
        let clothing = codes("clothing shop");
        assert_ne!(clothing[0], "14110", "{clothing:?}");
        assert_eq!(clothing.last(), Some(&"14110"), "{clothing:?}");
    }

    #[test]
    fn nothing_matches_nonsense() {
        assert!(codes("zzzzqqq").is_empty());
        assert!(codes("   ").is_empty());
        assert!(codes("the and of").is_empty());
    }
}

//! RE2 pattern constraints (§21.8): syntax checking, whole-value matching,
//! and the containment decision procedure backing MergeScalar's checked
//! pattern replacement (§20.3, E223).
//!
//! The `regex` crate approximates the specification's RE2 syntax pin: like
//! RE2 it excludes backreferences and lookaround, so every accepted pattern
//! denotes a true regular language and containment is decidable. Patterns
//! are matched against the entire value text (implicit anchoring, as if
//! `\A(?:pattern)\z`).
//!
//! Containment is decided by the recommended product-of-DFA reachability
//! construction (§21.8): `L(⋂new) ⊆ L(old)` fails iff some input is
//! accepted by every DFA of `new` but rejected by `old`'s — i.e. iff a
//! product state whose `new` components are all EOI-accepting and whose
//! `old` component is not is reachable from the anchored start. The DFAs
//! come from `regex-automata`; the walk is bounded by a state budget and
//! FAILS CLOSED (`Err`, surfaced as E223) when the budget is exceeded or a
//! pattern cannot be compiled to a DFA (e.g. Unicode word boundaries),
//! never silently accepting.

use regex_automata::{
    dfa::{dense, Automaton, StartKind},
    util::{primitives::StateID, start},
    Anchored,
};

/// Maximum number of visited product states before containment is deemed
/// not provable (fail closed). Generous for schema-sized patterns: the
/// product of a handful of small DFAs stays far below this.
const STATE_BUDGET: usize = 100_000;

/// Check that `pattern` is a valid pattern in the supported (RE2-style)
/// syntax. E222 on failure.
pub fn check_syntax(pattern: &str) -> Result<(), String> {
    regex::Regex::new(&anchored(pattern)).map(|_| ()).map_err(|error| error.to_string())
}

/// Whether `value` (in its entirety) matches `pattern` (§21.8). `Err` when
/// the pattern does not compile — a schema error (E222) reported
/// elsewhere, so callers skip rather than double-report.
pub fn matches_whole(pattern: &str, value: &str) -> Result<bool, String> {
    // Compiled per call for clarity, not speed: this is the reference
    // implementation, and pattern counts per scalar are small.
    let re = regex::Regex::new(&anchored(pattern)).map_err(|error| error.to_string())?;
    Ok(re.is_match(value))
}

fn anchored(pattern: &str) -> String {
    format!(r"\A(?:{})\z", pattern)
}

/// Decide `L(⋂new) ⊆ L(old)`: whether every text matched by all of the
/// `new` patterns is also matched by `old`. This is the per-inherited-
/// pattern decomposition of MergeScalar's replacement premise
/// `L(⋂new) ⊆ L(⋂old)` (§20.3).
pub fn intersection_contained(new: &[String], old: &str) -> Result<bool, String> {
    let new_dfas: Vec<dense::DFA<Vec<u32>>> = new.iter()
        .map(|pattern| build_dfa(pattern))
        .collect::<Result<_, _>>()?;
    let old_dfa = build_dfa(old)?;

    let config = start::Config::new().anchored(Anchored::Yes);
    let new_starts: Vec<StateID> = new_dfas.iter()
        .map(|dfa| dfa.start_state(&config).map_err(|error| error.to_string()))
        .collect::<Result<_, _>>()?;
    let old_start = old_dfa.start_state(&config).map_err(|error| error.to_string())?;

    // Breadth-first search over the product automaton. A state is a
    // vector of `new` DFA states plus the `old` DFA state.
    let mut visited: std::collections::HashSet<Vec<StateID>> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<(Vec<StateID>, StateID)> =
        std::collections::VecDeque::new();

    let key = |news: &[StateID], old_state: StateID| {
        let mut k = news.to_vec();
        k.push(old_state);
        k
    };

    visited.insert(key(&new_starts, old_start));
    queue.push_back((new_starts, old_start));

    while let Some((news, old_state)) = queue.pop_front() {
        if visited.len() > STATE_BUDGET {
            return Err(format!("state budget of {} product states exceeded", STATE_BUDGET));
        }

        // Acceptance at end-of-input: a witness of non-containment is a
        // product state where every `new` DFA accepts and `old` does not.
        let all_new_accept = news.iter().zip(&new_dfas).all(|(state, dfa)| {
            dfa.is_match_state(dfa.next_eoi_state(*state))
        });
        if all_new_accept && !old_dfa.is_match_state(old_dfa.next_eoi_state(old_state)) {
            return Ok(false);
        }

        for byte in 0u8..=255 {
            let mut next_news = Vec::with_capacity(news.len());
            let mut intersection_alive = true;
            for (state, dfa) in news.iter().zip(&new_dfas) {
                let next = dfa.next_state(*state, byte);
                if dfa.is_quit_state(next) {
                    return Err("pattern uses a feature the DFA cannot decide".to_string());
                }
                if dfa.is_dead_state(next) {
                    intersection_alive = false;
                    break;
                }
                next_news.push(next);
            }
            // A dead `new` component kills the intersection on this path;
            // a dead `old` component must still be explored (it is exactly
            // where witnesses live).
            if !intersection_alive { continue; }

            let next_old = old_dfa.next_state(old_state, byte);
            if old_dfa.is_quit_state(next_old) {
                return Err("pattern uses a feature the DFA cannot decide".to_string());
            }

            let k = key(&next_news, next_old);
            if visited.insert(k) {
                queue.push_back((next_news, next_old));
            }
        }
    }

    Ok(true)
}

fn build_dfa(pattern: &str) -> Result<dense::DFA<Vec<u32>>, String> {
    dense::Builder::new()
        .configure(dense::DFA::config().start_kind(StartKind::Anchored))
        .build(&anchored_dfa(pattern))
        .map_err(|error| error.to_string())
}

// The DFA side needs explicit end anchoring inside the pattern; the
// anchored start configuration supplies `\A`.
fn anchored_dfa(pattern: &str) -> String {
    format!(r"(?:{})\z", pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contained(new: &[&str], old: &str) -> bool {
        let new: Vec<String> = new.iter().map(|s| s.to_string()).collect();
        intersection_contained(&new, old).expect("containment should be decidable")
    }

    #[test]
    fn syntax_accepts_re2_and_rejects_backreferences() {
        assert!(check_syntax("[A-Z]{2}-[0-9]{4}").is_ok());
        assert!(check_syntax("(?i)hello|world").is_ok());
        assert!(check_syntax("(a)\\1").is_err(), "backreferences are not RE2");
        assert!(check_syntax("(?=x)y").is_err(), "lookahead is not RE2");
        assert!(check_syntax("[").is_err());
    }

    #[test]
    fn whole_value_matching_is_anchored() {
        assert!(matches_whole("[a-z]+", "hello").unwrap());
        assert!(!matches_whole("[a-z]+", "hello world").unwrap());
        assert!(!matches_whole("b+", "abba").unwrap(), "no substring matching");
    }

    #[test]
    fn containment_is_reflexive() {
        assert!(contained(&["[a-z]{3}"], "[a-z]{3}"));
    }

    #[test]
    fn literal_is_contained_in_class() {
        assert!(contained(&["abc"], "[a-z]+"));
        assert!(!contained(&["[a-z]+"], "abc"));
    }

    #[test]
    fn disjoint_languages_are_not_contained() {
        assert!(!contained(&["[0-9]+"], "[a-z]+"));
    }

    #[test]
    fn star_and_alternation_cases() {
        assert!(contained(&["(EU|UK)-[0-9]{4}"], "[A-Z]{2}-[0-9]{4}"));
        assert!(!contained(&["(EU|USA)-[0-9]{4}"], "[A-Z]{2}-[0-9]{4}"));
        assert!(contained(&["a*b"], "a*b?c?"));
    }

    #[test]
    fn intersection_narrows_below_any_single_pattern() {
        // ⋂{[a-z]+, .{2}} = two lowercase letters ⊆ [a-z]{2,3}, though
        // neither component alone is contained.
        assert!(contained(&["[a-z]+", ".{2}"], "[a-z]{2,3}"));
        assert!(!contained(&["[a-z]+"], "[a-z]{2,3}"));
    }

    #[test]
    fn empty_intersection_is_contained_in_everything() {
        assert!(contained(&["[0-9]+", "[a-z]+"], "x"));
    }
}

//! String similarity matching using Levenshtein edit distance.

use strsim::levenshtein;

/// Finds the candidate nearest to `input` by edit distance.
///
/// Accepts a candidate only when its distance is at most half of `input`'s
/// character count, rounded up, with a minimum threshold of 1.
///
/// Ties keep iterator order through [`Iterator::min_by_key`].
pub(crate) fn closest_match<'a, T>(
    candidates: impl Iterator<Item = (T, &'a str)>,
    input: &str,
) -> Option<T> {
    let threshold = input.chars().count().div_ceil(2).max(1);
    candidates
        .map(|(item, name)| (item, levenshtein(input, name)))
        .min_by_key(|&(_, distance)| distance)
        .filter(|&(_, distance)| distance <= threshold)
        .map(|(item, _)| item)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn matches_within_the_half_length_threshold() {
        let candidates = ["path", "name", "folder"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "nam"),
            Some("name")
        );
    }

    #[test]
    fn accepts_a_match_exactly_at_the_threshold() {
        let candidates = ["abc"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "ab"),
            Some("abc")
        );
    }

    #[test]
    fn rejects_a_match_past_the_threshold() {
        let candidates = ["name"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "na"),
            None
        );
    }

    #[test]
    fn uses_a_minimum_threshold_of_one_for_empty_input() {
        let candidates = ["a"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), ""),
            Some("a")
        );
    }

    #[test]
    fn returns_none_for_an_empty_candidate_list() {
        assert_eq!(
            closest_match(std::iter::empty::<(&str, &str)>(), "name"),
            None
        );
    }

    #[test]
    fn breaks_ties_by_iteration_order() {
        let candidates = ["cat", "bat"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "mat"),
            Some("cat")
        );
    }
}

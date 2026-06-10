//! Topic pattern matching for the Event Bus.
//!
//! Supports:
//! - **Exact match**: `"alpha.user.input.text"` matches only itself.
//! - **Single-segment wildcard**: `"alpha.user.*"` matches `"alpha.user.input"`
//!   but NOT `"alpha.user.input.text"` (no recursive matching).
//!
//! Rules:
//! - `*` matches exactly one dot-separated segment.
//! - No recursive wildcards (`**`).
//! - No regex.
//! - Patterns and topics are dot-separated, lowercase.

/// Check if a topic matches a subscription pattern.
///
/// # Examples
///
/// ```
/// use alpha_event_bus::matcher::matches_topic;
///
/// assert!(matches_topic("alpha.user.input", "alpha.user.input"));    // exact
/// assert!(matches_topic("alpha.user.input", "alpha.user.*"));        // wildcard
/// assert!(!matches_topic("alpha.user.input.text", "alpha.user.*"));  // too deep
/// assert!(matches_topic("alpha.user.input.text", "alpha.user.input.*")); // one level
/// assert!(!matches_topic("alpha.user.input", "alpha.system.*"));     // wrong prefix
/// ```
pub fn matches_topic(topic: &str, pattern: &str) -> bool {
    let topic_segments: Vec<&str> = topic.split('.').collect();
    let pattern_segments: Vec<&str> = pattern.split('.').collect();

    // Must have the same number of segments.
    if topic_segments.len() != pattern_segments.len() {
        return false;
    }

    // Compare segment by segment.
    for (t, p) in topic_segments.iter().zip(pattern_segments.iter()) {
        if *p == "*" {
            // Wildcard matches any single segment.
            continue;
        }
        if t != p {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(matches_topic("alpha.system.started", "alpha.system.started"));
    }

    #[test]
    fn test_exact_no_match() {
        assert!(!matches_topic("alpha.system.started", "alpha.system.shutdown"));
    }

    #[test]
    fn test_wildcard_single_segment() {
        assert!(matches_topic("alpha.user.input", "alpha.user.*"));
        assert!(matches_topic("alpha.user.voice", "alpha.user.*"));
    }

    #[test]
    fn test_wildcard_does_not_match_deeper() {
        // alpha.user.* should NOT match alpha.user.input.text (4 segments vs 3)
        assert!(!matches_topic("alpha.user.input.text", "alpha.user.*"));
    }

    #[test]
    fn test_wildcard_at_different_position() {
        assert!(matches_topic("alpha.system.started", "alpha.*.started"));
        assert!(matches_topic("alpha.aris.started", "alpha.*.started"));
        assert!(!matches_topic("alpha.system.shutdown", "alpha.*.started"));
    }

    #[test]
    fn test_multiple_wildcards() {
        assert!(matches_topic("alpha.user.input", "*.*.*"));
        assert!(!matches_topic("alpha.user.input.text", "*.*.*"));
    }

    #[test]
    fn test_no_match_different_lengths() {
        assert!(!matches_topic("alpha.system", "alpha.system.started"));
        assert!(!matches_topic("alpha.system.started", "alpha.system"));
    }

    #[test]
    fn test_empty_strings() {
        assert!(matches_topic("", ""));
        assert!(!matches_topic("alpha", ""));
        assert!(!matches_topic("", "alpha"));
    }

    #[test]
    fn test_wildcard_four_segments() {
        assert!(matches_topic(
            "alpha.user.input.text",
            "alpha.user.input.*"
        ));
        assert!(matches_topic(
            "alpha.user.input.voice",
            "alpha.user.input.*"
        ));
        assert!(!matches_topic(
            "alpha.user.input",
            "alpha.user.input.*"
        ));
    }
}

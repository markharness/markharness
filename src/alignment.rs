//! The `Spec-Reviewed` commit trailer (ADR 0019) and the rule that decides
//! when a recorded confirmation still applies (v2 design §5.3).
//!
//! A confirmation is bound to a **pair** — one Requirement and one TestCase —
//! at the commit that recorded it. Editing either side afterwards invalidates
//! that pair's confirmation, because what a human looked at is no longer what
//! the range contains.

use std::collections::BTreeSet;

/// The trailer key. ADR 0019 leaves the exact spelling to implementation;
/// this is it.
pub const TRAILER_KEY: &str = "Spec-Reviewed";

/// The only recognized reason today. Unknown reasons are not silently
/// accepted: a trailer whose reason this reader does not understand cannot
/// be counted as a confirmation (ADR 0019's "confirmed" is a claim about a
/// human judgement, not about a string being present).
pub const REASON_NO_CHANGE_REQUIRED: &str = "no-change-required";

/// One parsed `Spec-Reviewed` trailer, still carrying display ids: a
/// display id only means something at the commit that wrote it, so
/// resolution to UIDs happens against that commit, not here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParsedTrailer {
    pub requirement_id: String,
    pub case_id: String,
    pub reason: String,
}

/// Why a trailer line was not accepted. Kept so `impact` can explain why a
/// trailer the author wrote did not produce a confirmation, rather than
/// silently dropping it and reporting "unconfirmed" with no reason.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RejectedTrailer {
    pub line: String,
    pub reason: TrailerRejection,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrailerRejection {
    /// AC12/AC16: without both ids, a commit touching several Requirements
    /// or TestCases gives no way to tell which pair was confirmed.
    MissingTarget,
    UnknownReason(String),
    UnknownKey(String),
}

impl std::fmt::Display for TrailerRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrailerRejection::MissingTarget => write!(
                f,
                "needs both `requirement=<id>` and `case=<id>`; a trailer without a target cannot say which pair was confirmed"
            ),
            TrailerRejection::UnknownReason(reason) => {
                write!(
                    f,
                    "unknown reason `{reason}`; expected `{REASON_NO_CHANGE_REQUIRED}`"
                )
            }
            TrailerRejection::UnknownKey(key) => write!(f, "unknown key `{key}`"),
        }
    }
}

/// Reads every `Spec-Reviewed` trailer in one commit message.
///
/// Scans the whole message, not only its final lines: a squash merge embeds
/// the original commits' trailers partway through the merge commit's body
/// (ADR 0019). Multiple confirmations are written as multiple lines — a
/// single comma-separated line could not express one pair being invalidated
/// while another stays valid.
pub fn parse_trailers(message: &str) -> (Vec<ParsedTrailer>, Vec<RejectedTrailer>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for raw_line in message.lines() {
        // The key must start the line, with no leading whitespace. An
        // indented mention is prose or a fenced code block quoting the
        // syntax — documentation about a trailer, not a declaration of one.
        // Git's own trailer parsing takes the same position.
        let line = raw_line.trim_end();
        let Some(value) = line.strip_prefix(TRAILER_KEY) else {
            continue;
        };
        let Some(value) = value.strip_prefix(':') else {
            continue;
        };
        match parse_trailer_value(value.trim()) {
            Ok(trailer) => accepted.push(trailer),
            Err(reason) => rejected.push(RejectedTrailer {
                line: line.to_string(),
                reason,
            }),
        }
    }
    (accepted, rejected)
}

fn parse_trailer_value(value: &str) -> Result<ParsedTrailer, TrailerRejection> {
    let mut requirement_id = None;
    let mut case_id = None;
    let mut reason = None;
    for field in value.split_whitespace() {
        let Some((key, field_value)) = field.split_once('=') else {
            return Err(TrailerRejection::UnknownKey(field.to_string()));
        };
        match key {
            "requirement" => requirement_id = Some(field_value.to_string()),
            "case" => case_id = Some(field_value.to_string()),
            "reason" => reason = Some(field_value.to_string()),
            other => return Err(TrailerRejection::UnknownKey(other.to_string())),
        }
    }
    let reason = reason.unwrap_or_else(|| REASON_NO_CHANGE_REQUIRED.to_string());
    if reason != REASON_NO_CHANGE_REQUIRED {
        return Err(TrailerRejection::UnknownReason(reason));
    }
    match (requirement_id, case_id) {
        (Some(requirement_id), Some(case_id))
            if !requirement_id.is_empty() && !case_id.is_empty() =>
        {
            Ok(ParsedTrailer {
                requirement_id,
                case_id,
                reason,
            })
        }
        _ => Err(TrailerRejection::MissingTarget),
    }
}

/// A confirmation recorded at one commit, already resolved to the pair it
/// binds. `Pair` is deliberately the unit: a confirmation is never reused
/// for another pair, nor extended to a case added later (ADR 0019, AC30/AC31).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Confirmation {
    pub commit: String,
    pub requirement_uid: String,
    pub case_uid: String,
}

/// The Requirement UIDs and Case UIDs one commit altered the content of.
pub type ChangedUids = (BTreeSet<String>, BTreeSet<String>);

/// Keeps only the confirmations still valid at the end of the range.
///
/// v2 design §5.3 rule 2: if a later commit in the same range changes the
/// effective content of either side of a pair, that pair's confirmation is
/// void. `changed_after` gives, per commit, the Requirement UIDs and Case
/// UIDs whose content that commit altered.
pub fn surviving_confirmations(
    ordered_commits: &[String],
    confirmations: &[Confirmation],
    changed_after: &dyn Fn(&str) -> ChangedUids,
) -> Vec<Confirmation> {
    let position = |commit: &str| ordered_commits.iter().position(|c| c == commit);
    let mut surviving = Vec::new();
    for confirmation in confirmations {
        let Some(at) = position(&confirmation.commit) else {
            continue;
        };
        let invalidated = ordered_commits.iter().skip(at + 1).any(|later| {
            let (requirements, cases) = changed_after(later);
            requirements.contains(&confirmation.requirement_uid)
                || cases.contains(&confirmation.case_uid)
        });
        if !invalidated {
            surviving.push(confirmation.clone());
        }
    }
    surviving
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_trailer_naming_both_sides() {
        let (accepted, rejected) = parse_trailers(
            "fix: tighten the login rule\n\nSpec-Reviewed: requirement=req-login-01 case=case-login-success reason=no-change-required\n",
        );
        assert!(rejected.is_empty(), "{rejected:?}");
        assert_eq!(
            accepted,
            vec![ParsedTrailer {
                requirement_id: "req-login-01".to_string(),
                case_id: "case-login-success".to_string(),
                reason: REASON_NO_CHANGE_REQUIRED.to_string(),
            }]
        );
    }

    #[test]
    fn reads_several_confirmations_as_separate_lines() {
        let (accepted, _) = parse_trailers(
            "chore: touch two pairs\n\nSpec-Reviewed: requirement=r1 case=c1\nSpec-Reviewed: requirement=r2 case=c2\n",
        );
        assert_eq!(accepted.len(), 2);
        assert_eq!(accepted[1].case_id, "c2");
    }

    /// ADR 0019: a squash merge leaves the original trailers mid-body, so a
    /// reader that only inspects the final lines would miss them.
    #[test]
    fn finds_a_trailer_embedded_partway_through_a_squash_merge_body() {
        let (accepted, _) = parse_trailers(
            "Squashed PR #12\n\n* fix the rule\n\nSpec-Reviewed: requirement=r1 case=c1\n\n* follow-up tidy\n\nCo-Authored-By: Someone <nobody@example.com>\n",
        );
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].requirement_id, "r1");
    }

    /// AC12/AC16: without a target there is no pair to confirm.
    #[test]
    fn rejects_a_trailer_with_no_target() {
        let (accepted, rejected) = parse_trailers("Spec-Reviewed: reason=no-change-required\n");
        assert!(accepted.is_empty());
        assert_eq!(rejected[0].reason, TrailerRejection::MissingTarget);
    }

    #[test]
    fn rejects_a_one_sided_trailer() {
        let (accepted, rejected) = parse_trailers("Spec-Reviewed: requirement=r1\n");
        assert!(accepted.is_empty());
        assert_eq!(rejected[0].reason, TrailerRejection::MissingTarget);
    }

    #[test]
    fn rejects_an_unrecognized_reason() {
        let (accepted, rejected) =
            parse_trailers("Spec-Reviewed: requirement=r1 case=c1 reason=looks-fine\n");
        assert!(accepted.is_empty());
        assert_eq!(
            rejected[0].reason,
            TrailerRejection::UnknownReason("looks-fine".to_string())
        );
    }

    /// A trailer must begin the line. An indented copy is documentation
    /// about the syntax — a code block in a commit body explaining how to
    /// write one, or a quoted reply — not a declaration that a human
    /// confirmed anything.
    #[test]
    fn ignores_an_indented_mention_of_the_trailer() {
        let (accepted, rejected) = parse_trailers(
            "docs: explain the trailer

Write it like this:

    Spec-Reviewed: requirement=r1 case=c1
",
        );
        assert!(accepted.is_empty(), "{accepted:?}");
        assert!(rejected.is_empty(), "{rejected:?}");
    }

    #[test]
    fn ignores_a_trailer_inside_a_fenced_code_block_that_is_indented() {
        let (accepted, _) = parse_trailers(
            "docs: document the syntax

```text
	Spec-Reviewed: requirement=r1 case=c1
```
",
        );
        assert!(accepted.is_empty(), "{accepted:?}");
    }

    /// Trailing whitespace is invisible to the author, so it must not be
    /// what decides whether a confirmation counts.
    #[test]
    fn accepts_a_trailer_with_trailing_whitespace() {
        let (accepted, _) = parse_trailers(
            "fix: x

Spec-Reviewed: requirement=r1 case=c1   
",
        );
        assert_eq!(accepted.len(), 1);
    }

    #[test]
    fn ignores_unrelated_trailers() {
        let (accepted, rejected) =
            parse_trailers("feat: x\n\nCo-Authored-By: Someone <nobody@example.com>\n");
        assert!(accepted.is_empty());
        assert!(rejected.is_empty());
    }

    fn confirmation(commit: &str, requirement: &str, case: &str) -> Confirmation {
        Confirmation {
            commit: commit.to_string(),
            requirement_uid: requirement.to_string(),
            case_uid: case.to_string(),
        }
    }

    fn changes(
        map: Vec<(&'static str, Vec<&'static str>, Vec<&'static str>)>,
    ) -> impl Fn(&str) -> ChangedUids {
        move |commit: &str| {
            for (c, requirements, cases) in &map {
                if *c == commit {
                    return (
                        requirements.iter().map(|s| s.to_string()).collect(),
                        cases.iter().map(|s| s.to_string()).collect(),
                    );
                }
            }
            (BTreeSet::new(), BTreeSet::new())
        }
    }

    /// AC14: the Requirement changes again after being confirmed.
    #[test]
    fn a_later_change_to_the_requirement_voids_the_confirmation() {
        let commits = vec!["c1".to_string(), "c2".to_string()];
        let confirmations = vec![confirmation("c1", "r1", "a")];
        let changed = changes(vec![("c2", vec!["r1"], vec![])]);

        assert!(surviving_confirmations(&commits, &confirmations, &changed).is_empty());
    }

    /// AC29: the case changes again, even though the Requirement did not.
    #[test]
    fn a_later_change_to_the_case_voids_the_confirmation() {
        let commits = vec!["c1".to_string(), "c2".to_string()];
        let confirmations = vec![confirmation("c1", "r1", "a")];
        let changed = changes(vec![("c2", vec![], vec!["a"])]);

        assert!(surviving_confirmations(&commits, &confirmations, &changed).is_empty());
    }

    /// AC31: another case appearing later neither inherits the confirmation
    /// nor destroys it.
    #[test]
    fn an_unrelated_later_change_leaves_the_confirmation_standing() {
        let commits = vec!["c1".to_string(), "c2".to_string()];
        let confirmations = vec![confirmation("c1", "r1", "a")];
        let changed = changes(vec![("c2", vec![], vec!["b"])]);

        assert_eq!(
            surviving_confirmations(&commits, &confirmations, &changed),
            confirmations
        );
    }

    /// A change in a commit *before* the confirmation is what the human was
    /// looking at, so it cannot invalidate it.
    #[test]
    fn an_earlier_change_does_not_void_a_later_confirmation() {
        let commits = vec!["c1".to_string(), "c2".to_string()];
        let confirmations = vec![confirmation("c2", "r1", "a")];
        let changed = changes(vec![("c1", vec!["r1"], vec!["a"])]);

        assert_eq!(
            surviving_confirmations(&commits, &confirmations, &changed),
            confirmations
        );
    }
}

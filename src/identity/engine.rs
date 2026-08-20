use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::identity::{EntityKind, IdentityEvent, IdentityMutation};

/// Why replaying one entity's identity events failed (design doc §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    /// No event has an empty predecessor set.
    NoRootEvent,
    /// More than one event has an empty predecessor set — every entity
    /// has exactly one issuance/reissuance root.
    MultipleRootEvents(Vec<String>),
    /// The one root event's mutation is not `Issued`/`Reissued`.
    RootIsNotAnIssuance(String),
    /// An event references a predecessor UID absent from the input set.
    DanglingPredecessor {
        event_uid: String,
        missing_predecessor: String,
    },
    /// The predecessor graph contains a cycle (some events are never
    /// reachable from the root).
    CycleDetected,
    /// More than one event has no successor: two divergent heads exist
    /// with no `Resolved` event joining them (design doc §7).
    AmbiguousDivergence { divergent_head_uids: Vec<String> },
    /// A `Renamed` event's `from_id` disagrees with the id computed by
    /// replaying everything before it — the event log is internally
    /// inconsistent.
    InconsistentRename {
        event_uid: String,
        expected_from_id: String,
        actual_from_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdHistoryEntry {
    pub id: String,
    pub from_identity_event_uid: String,
}

/// The materialized state obtained by replaying one entity's identity
/// events (design doc §5's Registry entry, before it is written to disk).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub entity_uid: String,
    pub entity_kind: EntityKind,
    pub current_head_event_uid: String,
    pub status: Status,
    pub current_id: String,
    pub id_history: Vec<IdHistoryEntry>,
}

/// Replays every identity event belonging to one entity. `events` must
/// contain exactly that entity's events (callers filter by directory,
/// design doc §4.1); order does not matter — replay order is entirely
/// determined by `previous_identity_event_uid`/`previous_identity_event_uids`
/// (design doc §4.3), never by the order of this slice.
pub fn replay(entity_uid: &str, events: &[IdentityEvent]) -> Result<ReplayResult, ReplayError> {
    let by_uid: BTreeMap<&str, &IdentityEvent> = events
        .iter()
        .map(|e| (e.identity_event_uid.as_str(), e))
        .collect();

    for event in events {
        if let Some(p) = &event.previous_identity_event_uid
            && !by_uid.contains_key(p.as_str())
        {
            return Err(ReplayError::DanglingPredecessor {
                event_uid: event.identity_event_uid.clone(),
                missing_predecessor: p.clone(),
            });
        }
        for p in &event.previous_identity_event_uids {
            if !by_uid.contains_key(p.as_str()) {
                return Err(ReplayError::DanglingPredecessor {
                    event_uid: event.identity_event_uid.clone(),
                    missing_predecessor: p.clone(),
                });
            }
        }
    }

    let roots: Vec<&IdentityEvent> = events
        .iter()
        .filter(|e| {
            e.previous_identity_event_uid.is_none() && e.previous_identity_event_uids.is_empty()
        })
        .collect();
    let root = match roots.as_slice() {
        [] => return Err(ReplayError::NoRootEvent),
        [single] => *single,
        many => {
            return Err(ReplayError::MultipleRootEvents(
                many.iter().map(|e| e.identity_event_uid.clone()).collect(),
            ));
        }
    };
    if !root.mutation.can_be_root() {
        return Err(ReplayError::RootIsNotAnIssuance(
            root.identity_event_uid.clone(),
        ));
    }

    // children[uid] = events that name `uid` as a predecessor (singular or
    // plural). An event with no children is a head; more than one head
    // means an unresolved divergence.
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for event in events {
        if let Some(p) = &event.previous_identity_event_uid {
            children
                .entry(p.as_str())
                .or_default()
                .push(event.identity_event_uid.as_str());
        }
        for p in &event.previous_identity_event_uids {
            children
                .entry(p.as_str())
                .or_default()
                .push(event.identity_event_uid.as_str());
        }
    }

    let reachable = reachable_from_root(root, &by_uid, &children);
    if reachable.len() != events.len() {
        return Err(ReplayError::CycleDetected);
    }

    let heads: Vec<&str> = events
        .iter()
        .map(|e| e.identity_event_uid.as_str())
        .filter(|uid| children.get(uid).is_none_or(Vec::is_empty))
        .collect();
    let head_uid = match heads.as_slice() {
        [] => return Err(ReplayError::CycleDetected), // unreachable given the reachability check above
        [single] => *single,
        many => {
            let mut divergent_head_uids: Vec<String> = many.iter().map(|s| s.to_string()).collect();
            divergent_head_uids.sort();
            return Err(ReplayError::AmbiguousDivergence {
                divergent_head_uids,
            });
        }
    };

    let chain = winning_chain(by_uid[head_uid], &by_uid);
    apply_chain(entity_uid, &chain)
}

/// DFS over the predecessor graph starting at `root`, following
/// predecessor-of edges via `children`. Used only to confirm every input
/// event is reachable (anything left over after this is part of a cycle
/// disconnected from the root).
fn reachable_from_root<'a>(
    root: &'a IdentityEvent,
    by_uid: &BTreeMap<&'a str, &'a IdentityEvent>,
    children: &BTreeMap<&'a str, Vec<&'a str>>,
) -> std::collections::BTreeSet<&'a str> {
    let mut visited = std::collections::BTreeSet::new();
    let mut stack = vec![root.identity_event_uid.as_str()];
    while let Some(uid) = stack.pop() {
        if !visited.insert(uid) {
            continue;
        }
        if let Some(kids) = children.get(uid) {
            for kid in kids {
                if !visited.contains(kid) && by_uid.contains_key(kid) {
                    stack.push(kid);
                }
            }
        }
    }
    visited
}

/// Walks backward from the single head to the root, following
/// `winning_event_uid` through any `Resolved` event so the returned chain
/// contains only the branch that survives resolution (design doc §7).
fn winning_chain<'a>(
    head: &'a IdentityEvent,
    by_uid: &BTreeMap<&'a str, &'a IdentityEvent>,
) -> Vec<&'a IdentityEvent> {
    let mut chain = Vec::new();
    let mut current = head;
    loop {
        chain.push(current);
        match &current.mutation {
            IdentityMutation::Resolved { winning_event_uid } => {
                current = by_uid[winning_event_uid.as_str()];
            }
            _ => match &current.previous_identity_event_uid {
                Some(p) => current = by_uid[p.as_str()],
                None => break,
            },
        }
    }
    chain.reverse();
    chain
}

fn apply_chain(entity_uid: &str, chain: &[&IdentityEvent]) -> Result<ReplayResult, ReplayError> {
    let head = *chain
        .last()
        .expect("winning_chain always returns at least the root");
    let mut current_id: Option<String> = None;
    let mut id_history = Vec::new();
    let mut status = Status::Active;

    for event in chain {
        match &event.mutation {
            IdentityMutation::Issued { id } | IdentityMutation::Reissued { id, .. } => {
                current_id = Some(id.clone());
                id_history.push(IdHistoryEntry {
                    id: id.clone(),
                    from_identity_event_uid: event.identity_event_uid.clone(),
                });
            }
            IdentityMutation::Renamed { from_id, to_id } => {
                let expected = current_id.clone().unwrap_or_default();
                if &expected != from_id {
                    return Err(ReplayError::InconsistentRename {
                        event_uid: event.identity_event_uid.clone(),
                        expected_from_id: expected,
                        actual_from_id: from_id.clone(),
                    });
                }
                current_id = Some(to_id.clone());
                id_history.push(IdHistoryEntry {
                    id: to_id.clone(),
                    from_identity_event_uid: event.identity_event_uid.clone(),
                });
            }
            IdentityMutation::Retired => status = Status::Retired,
            IdentityMutation::Restored => status = Status::Active,
            IdentityMutation::Released { .. } | IdentityMutation::Resolved { .. } => {}
        }
    }

    Ok(ReplayResult {
        entity_uid: entity_uid.to_string(),
        entity_kind: head.entity_kind,
        current_head_event_uid: head.identity_event_uid.clone(),
        status,
        current_id: current_id.expect("winning_chain always starts at an Issued/Reissued root"),
        id_history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(uid: &str, previous: Option<&str>, mutation: IdentityMutation) -> IdentityEvent {
        IdentityEvent {
            identity_event_uid: uid.to_string(),
            entity_uid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: previous.map(str::to_string),
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T00:00:00Z".to_string(),
            mutation,
        }
    }

    #[test]
    fn replays_a_single_issuance_with_no_renames() {
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        );
        let result = replay("uid-1", &[issued]).unwrap();
        assert_eq!(result.current_id, "todo-management");
        assert_eq!(result.status, Status::Active);
        assert_eq!(result.id_history.len(), 1);
        assert_eq!(result.current_head_event_uid, "e0");
    }

    #[test]
    fn replays_a_linear_chain_of_renames() {
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        );
        let renamed = event(
            "e1",
            Some("e0"),
            IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "task-management".to_string(),
            },
        );
        let result = replay("uid-1", &[renamed, issued]).unwrap();
        assert_eq!(result.current_id, "task-management");
        assert_eq!(result.id_history.len(), 2);
        assert_eq!(result.id_history[0].id, "todo-management");
        assert_eq!(result.id_history[1].id, "task-management");
        assert_eq!(result.current_head_event_uid, "e1");
    }

    #[test]
    fn retire_then_restore_ends_active() {
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        );
        let retired = event("e1", Some("e0"), IdentityMutation::Retired);
        let restored = event("e2", Some("e1"), IdentityMutation::Restored);
        let result = replay("uid-1", &[issued, retired, restored]).unwrap();
        assert_eq!(result.status, Status::Active);
    }

    #[test]
    fn two_independent_renames_from_the_same_head_are_an_ambiguous_divergence() {
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        );
        let rename_a = event(
            "e1a",
            Some("e0"),
            IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "task-management".to_string(),
            },
        );
        let rename_b = event(
            "e1b",
            Some("e0"),
            IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "work-management".to_string(),
            },
        );
        let err = replay("uid-1", &[issued, rename_a, rename_b]).unwrap_err();
        assert_eq!(
            err,
            ReplayError::AmbiguousDivergence {
                divergent_head_uids: vec!["e1a".to_string(), "e1b".to_string()]
            }
        );
    }

    #[test]
    fn a_resolved_event_joins_a_divergence_and_replay_succeeds() {
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        );
        let rename_a = event(
            "e1a",
            Some("e0"),
            IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "task-management".to_string(),
            },
        );
        let rename_b = event(
            "e1b",
            Some("e0"),
            IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "work-management".to_string(),
            },
        );
        let mut resolved = event(
            "e2",
            None,
            IdentityMutation::Resolved {
                winning_event_uid: "e1a".to_string(),
            },
        );
        resolved.previous_identity_event_uids = vec!["e1a".to_string(), "e1b".to_string()];

        let result = replay("uid-1", &[issued, rename_a, rename_b, resolved]).unwrap();
        assert_eq!(result.current_id, "task-management");
        assert_eq!(result.current_head_event_uid, "e2");
    }

    #[test]
    fn dangling_predecessor_is_an_error() {
        let renamed = event(
            "e1",
            Some("missing"),
            IdentityMutation::Renamed {
                from_id: "a".to_string(),
                to_id: "b".to_string(),
            },
        );
        let err = replay("uid-1", &[renamed]).unwrap_err();
        assert_eq!(
            err,
            ReplayError::DanglingPredecessor {
                event_uid: "e1".to_string(),
                missing_predecessor: "missing".to_string()
            }
        );
    }

    #[test]
    fn no_events_is_a_missing_root_error() {
        assert_eq!(replay("uid-1", &[]).unwrap_err(), ReplayError::NoRootEvent);
    }

    #[test]
    fn two_roots_is_an_error() {
        let root_a = event(
            "e0a",
            None,
            IdentityMutation::Issued {
                id: "a".to_string(),
            },
        );
        let root_b = event(
            "e0b",
            None,
            IdentityMutation::Issued {
                id: "b".to_string(),
            },
        );
        let err = replay("uid-1", &[root_a, root_b]).unwrap_err();
        assert_eq!(
            err,
            ReplayError::MultipleRootEvents(vec!["e0a".to_string(), "e0b".to_string()])
        );
    }

    #[test]
    fn a_cycle_is_detected_rather_than_infinite_looping() {
        // e1 claims e2 as predecessor, e2 claims e1: neither is reachable
        // from any root, so no root exists among the two, and separately
        // this construction (no true root at all) is exercised via the
        // dangling/no-root paths above. Here we construct a cycle that
        // *is* reachable from a genuine root, to isolate cycle detection.
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "a".to_string(),
            },
        );
        let mut cyclic_a = event(
            "e1",
            Some("e2"),
            IdentityMutation::Renamed {
                from_id: "a".to_string(),
                to_id: "b".to_string(),
            },
        );
        let cyclic_b = event(
            "e2",
            Some("e1"),
            IdentityMutation::Renamed {
                from_id: "b".to_string(),
                to_id: "a".to_string(),
            },
        );
        // e1 and e2 form a 2-cycle disconnected from the real root `e0`.
        cyclic_a.previous_identity_event_uid = Some("e2".to_string());
        let err = replay("uid-1", &[issued, cyclic_a, cyclic_b]).unwrap_err();
        assert_eq!(err, ReplayError::CycleDetected);
    }

    #[test]
    fn inconsistent_rename_from_id_is_an_error() {
        let issued = event(
            "e0",
            None,
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        );
        let bad_rename = event(
            "e1",
            Some("e0"),
            IdentityMutation::Renamed {
                from_id: "wrong-id".to_string(),
                to_id: "task-management".to_string(),
            },
        );
        let err = replay("uid-1", &[issued, bad_rename]).unwrap_err();
        assert_eq!(
            err,
            ReplayError::InconsistentRename {
                event_uid: "e1".to_string(),
                expected_from_id: "todo-management".to_string(),
                actual_from_id: "wrong-id".to_string(),
            }
        );
    }
}

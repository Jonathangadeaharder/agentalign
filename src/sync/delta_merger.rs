use std::collections::HashSet;

use anyhow::Result;

/// Result of computing the delta between target, canonical, and local entries.
#[derive(Debug)]
pub struct DeltaResult {
    /// Keys in canonical but not in target — these should be added.
    pub entries_to_add: Vec<String>,
    /// Keys in both target and canonical with differing values.
    pub entries_to_update: Vec<String>,
    /// Keys in target but not in (canonical ∪ local) — these should be removed.
    /// Formula: Δ = C_target \ (C_canonical ∪ S_local)
    pub entries_to_remove: Vec<String>,
    /// Keys in target that are in S_local (user manual entries) — preserved.
    pub entries_preserved: Vec<String>,
}

/// Compute the delta between target config, canonical desired state, and user-preserved local entries.
///
/// # Algorithm
///
/// ```text
/// Δ = C_target \ (C_canonical ∪ S_local)
/// ```
///
/// Where:
/// - `C_target` = keys currently in the target file on disk
/// - `C_canonical` = keys the canonical config wants
/// - `S_local` = user-added keys NOT in canonical (to be preserved)
///
/// Returns the set of keys to remove, add, update, and preserve.
pub fn compute_delta(
    target: &serde_json::Value,
    canonical: &serde_json::Value,
    local_entries: &HashSet<String>,
) -> Result<DeltaResult> {
    let target_keys: HashSet<String> = target
        .as_object()
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();

    let canonical_keys: HashSet<String> = canonical
        .as_object()
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();

    // Δ = C_target \ (C_canonical ∪ S_local)
    let protected: HashSet<String> = canonical_keys.union(local_entries).cloned().collect();

    let entries_to_remove: Vec<String> = target_keys
        .difference(&protected)
        .cloned()
        .collect();

    let entries_to_add: Vec<String> = canonical_keys
        .difference(&target_keys)
        .cloned()
        .collect();

    // Keys in both — check if values differ for update candidates
    let entries_to_update: Vec<String> = target_keys
        .intersection(&canonical_keys)
        .filter(|k| target.get(k.as_str()) != canonical.get(k.as_str()))
        .cloned()
        .collect();

    // User-preserved entries: keys in target that are in local_entries but NOT canonical
    let entries_preserved: Vec<String> = target_keys
        .intersection(local_entries)
        .filter(|k| !canonical_keys.contains(k.as_str()))
        .cloned()
        .collect();

    Ok(DeltaResult {
        entries_to_add,
        entries_to_update,
        entries_to_remove,
        entries_preserved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_no_changes() {
        let target = json!({"a": 1, "b": 2});
        let canonical = json!({"a": 1, "b": 2});
        let local = HashSet::new();

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert!(result.entries_to_add.is_empty());
        assert!(result.entries_to_update.is_empty());
        assert!(result.entries_to_remove.is_empty());
        assert!(result.entries_preserved.is_empty());
    }

    #[test]
    fn test_removes_obsolete_keys() {
        let target = json!({"a": 1, "b": 2, "obsolete": true});
        let canonical = json!({"a": 1, "b": 2});
        let local = HashSet::new();

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert_eq!(result.entries_to_remove, vec!["obsolete"]);
        assert!(result.entries_preserved.is_empty());
    }

    #[test]
    fn test_preserves_local_entries() {
        let target = json!({"a": 1, "user_thing": "keep"});
        let canonical = json!({"a": 1});
        let mut local = HashSet::new();
        local.insert("user_thing".to_string());

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert!(result.entries_to_remove.is_empty());
        assert_eq!(result.entries_preserved, vec!["user_thing"]);
    }

    #[test]
    fn test_adds_new_canonical_keys() {
        let target = json!({"a": 1});
        let canonical = json!({"a": 1, "b": "new", "c": "also_new"});
        let local = HashSet::new();

        let result = compute_delta(&target, &canonical, &local).unwrap();
        let mut added: Vec<String> = result.entries_to_add.clone();
        added.sort();
        assert_eq!(added, vec!["b", "c"]);
    }

    #[test]
    fn test_updates_changed_values() {
        let target = json!({"a": 1, "b": "old"});
        let canonical = json!({"a": 1, "b": "new"});
        let local = HashSet::new();

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert_eq!(result.entries_to_update, vec!["b"]);
    }

    #[test]
    fn test_union_protection() {
        // A key in target but NOT in canonical AND NOT in local -> removed
        // A key in target and in local but NOT in canonical -> preserved
        let target = json!({"remove_me": 1, "keep_me": 2, "shared": 3});
        let canonical = json!({"shared": 3});
        let mut local = HashSet::new();
        local.insert("keep_me".to_string());

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert_eq!(result.entries_to_remove, vec!["remove_me"]);
        assert_eq!(result.entries_preserved, vec!["keep_me"]);
    }

    #[test]
    fn test_empty_target() {
        let target = json!({});
        let canonical = json!({"a": 1, "b": 2});
        let local = HashSet::new();

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert_eq!(result.entries_to_add.len(), 2);
        assert!(result.entries_to_remove.is_empty());
    }

    #[test]
    fn test_empty_canonical() {
        let target = json!({"a": 1, "b": 2});
        let canonical = json!({});
        let local = HashSet::new();

        let result = compute_delta(&target, &canonical, &local).unwrap();
        assert!(result.entries_to_add.is_empty());
        assert_eq!(result.entries_to_remove.len(), 2);
    }
}

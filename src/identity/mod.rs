//! The immutable identity model (ADR 0013,
//! `docs/ja/design/immutable-identity-model-design.md`): a `uid` that
//! persists across `id:` renames, layered on top of `knowledge/`'s
//! existing mutable, human-facing ids. Kept separate from `id_cache.rs`
//! (single-ref, id-only resolution) because its responsibility — identity
//! lifecycle, event replay, and crash-recoverable multi-file writes — is
//! substantially larger and orthogonal.

pub mod audit;
pub mod derived_uid;
pub mod engine;
pub mod entity_kind;
pub mod event;
pub mod feature_ops;
pub mod knowledge_walk;
pub mod lock;
pub mod marker;
pub mod migration_manifest;
pub mod recovery;
pub mod registry;

pub use audit::{AuditReport, AuditViolation, run_audit};
pub use engine::{IdHistoryEntry, ReplayError, ReplayResult, Status, replay};
pub use entity_kind::{EntityDescriptor, EntityKind, descriptor};
pub use event::{IdentityEvent, IdentityMutation};
pub use feature_ops::{
    MigrateError, MigrateReport, MigratedEntity, ReissueError, ReissuedEntity, ReleaseError,
    RenameError, ResolveError, RestoreError, RetireError, SyncError, migrate_entities,
    plan_migration, reissue_entity, release_id, rename_id, resolve_divergence, restore_entity,
    retire_entity, sync_entity,
};
pub use marker::{IDENTITY_SCHEMA_VERSION, is_uid_mode};
pub use migration_manifest::{
    AmbiguousCaseId, CrossBoundaryError, LegacyElementLocator, LegacySnapshot, Manifest,
    ManifestEntry, read as read_manifest, resolve_case_uid, resolve_case_uid_across_refs,
    resolve_case_uid_with_signature,
};

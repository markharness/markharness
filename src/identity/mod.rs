//! The immutable identity model (ADR 0013,
//! `docs/ja/design/immutable-identity-model-design.md`): a `uid` that
//! persists across `id:` renames, layered on top of `knowledge/`'s
//! existing mutable, human-facing ids. Kept separate from `id_cache.rs`
//! (single-ref, id-only resolution) because its responsibility — identity
//! lifecycle, event replay, and crash-recoverable multi-file writes — is
//! substantially larger and orthogonal.

pub mod derived_uid;
pub mod engine;
pub mod entity_kind;
pub mod event;
pub mod feature_ops;
pub mod lock;
pub mod recovery;
pub mod registry;

pub use engine::{IdHistoryEntry, ReplayError, ReplayResult, Status, replay};
pub use entity_kind::{EntityDescriptor, EntityKind, descriptor};
pub use event::{IdentityEvent, IdentityMutation};
pub use feature_ops::{
    MigrateError, MigrateReport, MigratedFeature, ReleaseError, RenameError, ResolveError,
    migrate_features, plan_feature_migration, release_id, rename_id, resolve_divergence,
};

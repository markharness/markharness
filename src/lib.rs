// Managed writes must go through fs_safety's symlink-safe primitives
// instead of calling std::fs::write/rename/remove_dir_all/remove_file or
// File::create directly (see clippy.toml). Only enforced outside cfg(test)
// so test fixtures can keep using std::fs freely for setup.
#![warn(clippy::disallowed_methods)]
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod application;
pub mod audit_scope;
pub mod axes;
pub mod backfill;
pub mod canonical;
pub mod changes;
pub mod cli;
pub mod derived_index;
pub mod execution;
pub mod fs_safety;
pub mod generate;
pub mod git;
pub mod id_cache;
pub mod identity;
pub mod init;
pub mod interactive;
pub mod knowledge;
pub mod knowledge_apply;
pub mod knowledge_draft;
pub mod knowledge_edit;
pub mod knowledge_source;
pub mod lineage;
pub mod milestone;
pub mod plan;
pub mod presentation;
pub mod project_root;
pub mod schema;
pub mod server;
pub mod traceability;
pub mod validate;
pub mod verify;

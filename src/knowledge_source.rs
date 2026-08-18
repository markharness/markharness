use std::io;
use std::path::{Component, Path, PathBuf};

use crate::fs_safety::replace_file;
use crate::generate::{self, KnowledgeSnapshot};
use crate::git::{self, ObjectKind};

/// Seam for loading one normalized Knowledge snapshot regardless of whether
/// its bytes come from the working tree or an immutable Git tree.
pub trait KnowledgeSource {
    fn load_snapshot(&self) -> io::Result<KnowledgeSnapshot>;
}

pub struct WorkingTreeKnowledgeSource {
    knowledge_root: PathBuf,
}

impl WorkingTreeKnowledgeSource {
    pub fn new(knowledge_root: impl Into<PathBuf>) -> Self {
        Self {
            knowledge_root: knowledge_root.into(),
        }
    }
}

impl KnowledgeSource for WorkingTreeKnowledgeSource {
    fn load_snapshot(&self) -> io::Result<KnowledgeSnapshot> {
        generate::load_knowledge_snapshot(&self.knowledge_root)
    }
}

pub struct GitTreeKnowledgeSource<'a> {
    repository_root: &'a Path,
    git_ref: &'a str,
}

impl<'a> GitTreeKnowledgeSource<'a> {
    pub fn new(repository_root: &'a Path, git_ref: &'a str) -> Self {
        Self {
            repository_root,
            git_ref,
        }
    }
}

impl KnowledgeSource for GitTreeKnowledgeSource<'_> {
    fn load_snapshot(&self) -> io::Result<KnowledgeSnapshot> {
        let staging = tempfile::tempdir()?;
        for entry in git::ls_tree_recursive(self.repository_root, self.git_ref, "knowledge")? {
            if entry.kind != ObjectKind::Blob {
                continue;
            }
            let relative = Path::new(&entry.path);
            if !relative.starts_with("knowledge")
                || relative
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsafe Git tree path: {}", entry.path),
                ));
            }
            let target = staging.path().join(relative);
            let content = git::show_blob_by_sha(self.repository_root, &entry.sha)?;
            replace_file(staging.path(), &target, content.as_bytes())?;
        }
        generate::load_knowledge_snapshot(&staging.path().join("knowledge"))
    }
}

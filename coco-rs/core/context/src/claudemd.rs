use std::path::Path;
use std::path::PathBuf;

/// A discovered CLAUDE.md file.
#[derive(Debug, Clone)]
pub struct ClaudeMdFile {
    pub path: PathBuf,
    pub content: String,
    pub source: ClaudeMdSource,
}

/// Where a CLAUDE.md was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeMdSource {
    UserGlobal,
    Project,
    ProjectRoot,
    Local,
    Parent,
    Child,
}

/// Discover all CLAUDE.md files for the given working directory.
/// Searches in priority order: user-global, project, project root, local, parents, children.
pub fn discover_claude_md_files(cwd: &Path) -> Vec<ClaudeMdFile> {
    let mut files = Vec::new();

    // 1. User global: ~/.coco/CLAUDE.md
    if let Some(home) = dirs_home() {
        let global_md = home.join(".coco").join("CLAUDE.md");
        try_load(&global_md, ClaudeMdSource::UserGlobal, &mut files);
    }

    // 2. .claude/CLAUDE.md (project config dir)
    let project_md = cwd.join(".claude").join("CLAUDE.md");
    try_load(&project_md, ClaudeMdSource::Project, &mut files);

    // 3. CLAUDE.md (project root)
    let root_md = cwd.join("CLAUDE.md");
    try_load(&root_md, ClaudeMdSource::ProjectRoot, &mut files);

    // 4. .claude/CLAUDE.local.md (local, gitignored)
    let local_md = cwd.join(".claude").join("CLAUDE.local.md");
    try_load(&local_md, ClaudeMdSource::Local, &mut files);

    // 5. Parent directories (walk up, max 10 levels)
    let mut parent = cwd.parent();
    let mut depth = 0;
    while let Some(dir) = parent {
        if depth >= 10 {
            break;
        }
        let parent_md = dir.join("CLAUDE.md");
        try_load(&parent_md, ClaudeMdSource::Parent, &mut files);
        parent = dir.parent();
        depth += 1;
    }

    // 6. Child directories (immediate children only, look for CLAUDE.md)
    if let Ok(entries) = std::fs::read_dir(cwd) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let child_md = entry.path().join("CLAUDE.md");
                try_load(&child_md, ClaudeMdSource::Child, &mut files);
            }
        }
    }

    files
}

fn try_load(path: &Path, source: ClaudeMdSource, files: &mut Vec<ClaudeMdFile>) {
    if path.exists() {
        // Avoid duplicates
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if files
            .iter()
            .any(|f| f.path.canonicalize().unwrap_or_else(|_| f.path.clone()) == canonical)
        {
            return;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            files.push(ClaudeMdFile {
                path: path.to_path_buf(),
                content,
                source,
            });
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
#[path = "claudemd.test.rs"]
mod tests;

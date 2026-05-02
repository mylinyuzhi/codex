use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

fn write_file(dir: &std::path::Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

#[test]
fn returns_empty_for_missing_dir() {
    let path = std::path::Path::new("/no/such/dir");
    assert!(scan_memory_files(path).is_empty());
}

#[test]
fn skips_memory_md_index_file() {
    let temp = tempdir().unwrap();
    write_file(
        temp.path(),
        "MEMORY.md",
        "# Memory Index\n\n- [a](a.md) — h\n",
    );
    write_file(
        temp.path(),
        "user_role.md",
        "---\nname: x\ndescription: d\ntype: user\n---\nbody\n",
    );
    let scanned = scan_memory_files(temp.path());
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].filename, "user_role.md");
}

#[test]
fn parses_frontmatter_and_sorts_newest_first() {
    let temp = tempdir().unwrap();
    write_file(
        temp.path(),
        "old.md",
        "---\nname: old\ndescription: old desc\ntype: user\n---\nbody\n",
    );
    // Force an earlier mtime on `old.md`.
    let mtime = filetime::FileTime::from_unix_time(1_000_000, 0);
    filetime::set_file_mtime(temp.path().join("old.md"), mtime).unwrap();

    write_file(
        temp.path(),
        "new.md",
        "---\nname: new\ndescription: new desc\ntype: feedback\n---\nbody\n",
    );

    let scanned = scan_memory_files(temp.path());
    assert_eq!(scanned.len(), 2);
    assert_eq!(scanned[0].filename, "new.md");
    assert_eq!(scanned[1].filename, "old.md");
    let fm = scanned[0].frontmatter.as_ref().unwrap();
    assert_eq!(fm.memory_type, crate::store::MemoryEntryType::Feedback);
}

#[test]
fn manifest_formats_each_entry() {
    let temp = tempdir().unwrap();
    write_file(
        temp.path(),
        "x.md",
        "---\nname: x\ndescription: short hook\ntype: project\n---\nbody\n",
    );
    let scanned = scan_memory_files(temp.path());
    let m = format_memory_manifest(&scanned);
    assert!(m.contains("[project] x.md"));
    assert!(m.contains("short hook"));
}

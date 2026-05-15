//! Tests for the skill change detector.

use super::*;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::time::Duration as TokioDuration;
use tokio::time::sleep;
use tokio::time::timeout;

/// Fast-config preset for tests so they finish in well under 5s
/// without sacrificing the per-knob coverage. Each duration is the
/// smallest multiple-of-50ms value that still lets the stability +
/// debounce stages observably do their job.
fn fast_config(ignored: Vec<PathBuf>) -> WatcherConfig {
    WatcherConfig {
        stability_threshold: TokioDuration::from_millis(150),
        poll_interval: TokioDuration::from_millis(80),
        debounce: TokioDuration::from_millis(50),
        ignored_dirs: ignored,
    }
}

#[test]
fn path_in_ignored_dir_matches_dot_git() {
    let ignored = vec![PathBuf::from(".git")];
    assert!(path_is_in_ignored_dir(
        Path::new("/repo/.git/HEAD"),
        &ignored
    ));
    assert!(path_is_in_ignored_dir(
        Path::new("/repo/sub/.git/refs/heads/main"),
        &ignored
    ));
    assert!(!path_is_in_ignored_dir(
        Path::new("/repo/.claude/skills/foo/SKILL.md"),
        &ignored
    ));
}

#[test]
fn path_in_ignored_dir_does_not_match_substring() {
    let ignored = vec![PathBuf::from(".git")];
    // `.gitignore` and `git/` are not the `.git` directory.
    assert!(!path_is_in_ignored_dir(
        Path::new("/repo/.gitignore"),
        &ignored
    ));
    assert!(!path_is_in_ignored_dir(
        Path::new("/repo/git/foo.md"),
        &ignored
    ));
}

#[test]
fn classify_keeps_only_md_paths() {
    let mut event = coco_file_watch::Event::new(coco_file_watch::EventKind::Any);
    event = event.add_path(PathBuf::from("/skills/foo/SKILL.md"));
    event = event.add_path(PathBuf::from("/skills/foo/notes.txt"));
    let result = classify_skill_event(&event, &[]).expect("event classified");
    assert_eq!(result.paths.len(), 1);
    assert_eq!(result.paths[0], PathBuf::from("/skills/foo/SKILL.md"));
}

#[test]
fn classify_filters_git_paths() {
    let ignored = vec![PathBuf::from(".git")];
    let mut event = coco_file_watch::Event::new(coco_file_watch::EventKind::Any);
    event = event.add_path(PathBuf::from("/repo/.git/HEAD"));
    event = event.add_path(PathBuf::from("/repo/.git/info/refs.md"));
    let result = classify_skill_event(&event, &ignored);
    assert!(
        result.is_none(),
        "expected no classified event when all paths are inside .git"
    );
}

#[test]
fn watcher_config_defaults_match_ts() {
    let cfg = WatcherConfig::default();
    assert_eq!(cfg.stability_threshold, Duration::from_millis(1000));
    assert_eq!(cfg.poll_interval, Duration::from_millis(2000));
    assert_eq!(cfg.debounce, Duration::from_millis(300));
    assert_eq!(cfg.ignored_dirs, vec![PathBuf::from(".git")]);
}

// -----------------------------------------------------------------------
// End-to-end integration via real filesystem events.
// -----------------------------------------------------------------------

fn write_skill(dir: &Path, name: &str, body: &str) -> PathBuf {
    let skill_dir = dir.join(name);
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    let path = skill_dir.join("SKILL.md");
    std::fs::write(&path, body).expect("write SKILL.md");
    path
}

#[tokio::test]
async fn stable_file_triggers_reload() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_dir = tmp.path().to_path_buf();

    let manager = Arc::new(SkillManager::new());
    let detector = SkillChangeDetector::with_config(
        Arc::clone(&manager),
        vec![skills_dir.clone()],
        Vec::new(),
        fast_config(vec![PathBuf::from(".git")]),
        None,
    )
    .expect("watcher");
    let mut rx = detector.subscribe();

    // Wait for the notify backend to actually register the watch.
    sleep(TokioDuration::from_millis(80)).await;

    // Write a single SKILL.md and stop touching it — stability
    // threshold should elapse and the bridge should emit.
    write_skill(
        &skills_dir,
        "alpha",
        "---\ndescription: alpha skill\n---\n# Body\n",
    );

    let event = timeout(TokioDuration::from_secs(4), rx.recv())
        .await
        .expect("watcher did not fire in time")
        .expect("recv error");
    assert!(!event.blocked_by_hook);
    assert!(
        !event.changed_paths.is_empty(),
        "expected at least one changed path"
    );
    // SkillManager should now know about `alpha`.
    assert!(manager.get("alpha").is_some(), "skill should be reloaded");
}

#[tokio::test]
async fn rapidly_modified_file_is_debounced() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_dir = tmp.path().to_path_buf();

    let manager = Arc::new(SkillManager::new());
    let detector = SkillChangeDetector::with_config(
        Arc::clone(&manager),
        vec![skills_dir.clone()],
        Vec::new(),
        fast_config(vec![PathBuf::from(".git")]),
        None,
    )
    .expect("watcher");
    let mut rx = detector.subscribe();

    sleep(TokioDuration::from_millis(80)).await;

    // Write the file repeatedly, well below the 150ms stability
    // threshold. The bridge must coalesce these into at most one
    // reload until writes stop.
    let path = skills_dir.join("beta").join("SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for i in 0..6 {
        std::fs::write(&path, format!("---\ndescription: rev {i}\n---\n# Body\n")).unwrap();
        sleep(TokioDuration::from_millis(40)).await;
    }
    // Now stop writing — give the stability threshold time to elapse
    // and the poller a sweep window.
    sleep(TokioDuration::from_millis(400)).await;

    // First recv MUST succeed.
    let first = timeout(TokioDuration::from_secs(2), rx.recv())
        .await
        .expect("first reload should fire")
        .expect("recv error");
    assert!(
        !first.changed_paths.is_empty(),
        "first reload missing paths"
    );

    // Any further events on the channel during a generous window must
    // either be empty or report the same file (we don't strictly
    // forbid a second reload here — notify can fan out close/atomic
    // rename pairs — but we ensure there are not many).
    let mut extra_events = 0;
    let drain_deadline = TokioDuration::from_millis(400);
    let start = Instant::now();
    while Instant::now().duration_since(start) < drain_deadline {
        match timeout(TokioDuration::from_millis(120), rx.recv()).await {
            Ok(Ok(_)) => extra_events += 1,
            Ok(Err(_)) | Err(_) => break,
        }
    }
    assert!(
        extra_events <= 1,
        "expected debounce to coalesce, got {extra_events} extra reloads"
    );
}

#[tokio::test]
async fn git_path_change_is_ignored() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_dir = tmp.path().to_path_buf();
    // Pre-create the .git dir BEFORE the watcher starts so notify
    // doesn't first see the directory-create event itself (some
    // backends report parent-dir mutation as the watched path).
    let git_dir = skills_dir.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create .git dir");

    let manager = Arc::new(SkillManager::new());
    let detector = SkillChangeDetector::with_config(
        Arc::clone(&manager),
        vec![skills_dir.clone()],
        Vec::new(),
        fast_config(vec![PathBuf::from(".git")]),
        None,
    )
    .expect("watcher");
    let mut rx = detector.subscribe();

    sleep(TokioDuration::from_millis(80)).await;

    // Touch a .md inside .git/ — the path filter must drop it.
    std::fs::write(git_dir.join("HEAD.md"), b"deadbeef").expect("write HEAD");

    // Generous wait so the stability poller would have fired twice.
    let result = timeout(TokioDuration::from_millis(800), rx.recv()).await;
    assert!(
        result.is_err(),
        "expected no event for .git/* changes, got {result:?}"
    );
}

#[tokio::test]
async fn additional_dirs_are_watched() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let primary_dir = tmp.path().join("primary");
    let extra_dir = tmp.path().join("extra");
    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&extra_dir).unwrap();

    let manager = Arc::new(SkillManager::new());
    let detector = SkillChangeDetector::with_config(
        Arc::clone(&manager),
        vec![primary_dir.clone()],
        vec![extra_dir.clone()],
        fast_config(vec![PathBuf::from(".git")]),
        None,
    )
    .expect("watcher");

    // Both roots should be reported in watched_dirs.
    assert!(detector.watched_dirs().contains(&primary_dir));
    assert!(detector.watched_dirs().contains(&extra_dir));

    let mut rx = detector.subscribe();
    sleep(TokioDuration::from_millis(80)).await;

    // Drop a skill into the *additional* dir.
    write_skill(
        &extra_dir,
        "gamma",
        "---\ndescription: gamma skill\n---\n# Body\n",
    );

    let event = timeout(TokioDuration::from_secs(4), rx.recv())
        .await
        .expect("watcher did not fire for additional dir")
        .expect("recv error");
    assert!(
        !event.changed_paths.is_empty(),
        "no changed paths from additional dir"
    );
    assert!(
        manager.get("gamma").is_some(),
        "skill from additional dir should be loaded"
    );
}

// -----------------------------------------------------------------------
// ConfigChange hook integration.
// -----------------------------------------------------------------------

struct CountingDispatcher {
    calls: AtomicUsize,
    block: bool,
}

#[async_trait]
impl ConfigChangeHookDispatcher for CountingDispatcher {
    async fn dispatch_skills_change(&self, _path: &Path) -> Result<bool, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.block)
    }
}

#[tokio::test]
async fn config_change_hook_fires_before_reload() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_dir = tmp.path().to_path_buf();
    let dispatcher = Arc::new(CountingDispatcher {
        calls: AtomicUsize::new(0),
        block: false,
    });

    let manager = Arc::new(SkillManager::new());
    let detector = SkillChangeDetector::with_config(
        Arc::clone(&manager),
        vec![skills_dir.clone()],
        Vec::new(),
        fast_config(vec![PathBuf::from(".git")]),
        Some(dispatcher.clone()),
    )
    .expect("watcher");
    let mut rx = detector.subscribe();

    sleep(TokioDuration::from_millis(80)).await;
    write_skill(
        &skills_dir,
        "delta",
        "---\ndescription: delta\n---\n# Body\n",
    );

    let event = timeout(TokioDuration::from_secs(4), rx.recv())
        .await
        .expect("watcher did not fire")
        .expect("recv error");
    assert!(!event.blocked_by_hook);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert!(manager.get("delta").is_some());
}

#[tokio::test]
async fn config_change_hook_can_block_reload() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_dir = tmp.path().to_path_buf();
    let dispatcher = Arc::new(CountingDispatcher {
        calls: AtomicUsize::new(0),
        block: true,
    });

    let manager = Arc::new(SkillManager::new());
    let detector = SkillChangeDetector::with_config(
        Arc::clone(&manager),
        vec![skills_dir.clone()],
        Vec::new(),
        fast_config(vec![PathBuf::from(".git")]),
        Some(dispatcher.clone()),
    )
    .expect("watcher");
    let mut rx = detector.subscribe();

    sleep(TokioDuration::from_millis(80)).await;
    write_skill(
        &skills_dir,
        "epsilon",
        "---\ndescription: epsilon\n---\n# Body\n",
    );

    let event = timeout(TokioDuration::from_secs(4), rx.recv())
        .await
        .expect("watcher did not fire")
        .expect("recv error");
    assert!(
        event.blocked_by_hook,
        "blocking dispatcher should mark the event"
    );
    assert!(
        manager.get("epsilon").is_none(),
        "skill must NOT be loaded when the hook blocks the reload"
    );
}

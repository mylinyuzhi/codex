//! Binary dispatcher for cocode CLI.
//!
//! This crate provides the "arg0 trick" for single-binary deployment:
//! - Dispatches to specialized CLIs based on executable name (argv[0])
//! - Hijacks apply_patch execution via secret flag (argv[1])
//! - Sets up PATH with symlinks for subprocess integration
//!
//! # Architecture
//!
//! When the cocode binary is invoked:
//!
//! 1. **argv[0] dispatch**: If the executable name is `apply_patch`, `applypatch`,
//!    or `cocode-linux-sandbox`, dispatch directly to those implementations.
//!
//! 2. **argv[1] hijack**: If the first argument is `--cocode-run-as-apply-patch`,
//!    run apply_patch with the second argument as the patch.
//!
//! 3. **Normal flow**: Load dotenv, set up PATH with symlinks, and run main_fn.
//!
//! # Example
//!
//! ```ignore
//! use cocode_arg0::arg0_dispatch_or_else;
//!
//! fn main() -> ExitCode {
//!     arg0_dispatch_or_else(|arg0_paths| async move {
//!         // arg0_paths.cocode_linux_sandbox_exe is available on Linux
//!         Ok(())
//!     })
//! }
//! ```

use std::fs::File;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::symlink;
use tempfile::TempDir;

/// The secret argument used to hijack apply_patch invocation.
pub const COCODE_APPLY_PATCH_ARG1: &str = "--cocode-run-as-apply-patch";

/// The name of the Linux sandbox executable (arg0).
const LINUX_SANDBOX_ARG0: &str = "cocode-linux-sandbox";

/// The name of the apply_patch executable (arg0).
const APPLY_PATCH_ARG0: &str = "apply_patch";

/// Alternate spelling of apply_patch.
const MISSPELLED_APPLY_PATCH_ARG0: &str = "applypatch";

/// Environment variable prefix that cannot be set via .env files (security).
const ILLEGAL_ENV_VAR_PREFIX: &str = "COCODE_";

/// Lock file name used to coordinate janitor cleanup of stale temp dirs.
const LOCK_FILENAME: &str = ".lock";

/// Tokio worker thread stack size (16 MB). Prevents stack overflow on deep
/// async call stacks (e.g. nested JSON processing, recursive tool execution).
const TOKIO_WORKER_STACK_SIZE_BYTES: usize = 16 * 1024 * 1024;

/// Paths to helper executables discovered during arg0 dispatch.
///
/// Passed to the main async entry point so downstream code can locate helpers
/// without scanning PATH.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Arg0DispatchPaths {
    pub cocode_linux_sandbox_exe: Option<PathBuf>,
}

/// Keeps the per-session PATH entry alive and locked for the process lifetime.
///
/// The embedded file lock prevents the janitor from removing the temp directory
/// while this process is still running.
pub struct Arg0PathEntryGuard {
    _temp_dir: TempDir,
    _lock_file: File,
    paths: Arg0DispatchPaths,
}

impl Arg0PathEntryGuard {
    fn new(temp_dir: TempDir, lock_file: File, paths: Arg0DispatchPaths) -> Self {
        Self {
            _temp_dir: temp_dir,
            _lock_file: lock_file,
            paths,
        }
    }

    pub fn paths(&self) -> &Arg0DispatchPaths {
        &self.paths
    }
}

/// Perform arg0 dispatch and setup, returning a guard for the PATH entry.
///
/// This function:
/// 1. Checks argv[0] for special executable names and dispatches accordingly
/// 2. Checks argv[1] for the apply_patch hijack flag
/// 3. Loads dotenv from ~/.cocode/.env
/// 4. Creates a temp directory with symlinks and prepends it to PATH
///
/// Returns `Some(Arg0PathEntryGuard)` if PATH was set up, `None` if setup failed
/// but we can proceed. Never returns if dispatched to a specialized CLI.
pub fn arg0_dispatch() -> Option<Arg0PathEntryGuard> {
    // Determine if we were invoked via a special alias.
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();
    let exe_name = Path::new(&argv0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // argv[0] dispatch: specialized CLIs (never returns)
    if exe_name == LINUX_SANDBOX_ARG0 {
        // Sandbox invocation when sandbox is not yet fully implemented.
        // In non-sandbox mode (default), this shouldn't be called.
        // Log a warning and exit gracefully - sandbox is optional.
        //
        // In a full implementation, this would call cocode_sandbox::run_main()
        // to apply Landlock/Seatbelt restrictions before execvp().
        eprintln!(
            "Warning: {LINUX_SANDBOX_ARG0} invoked but sandbox enforcement is not yet implemented."
        );
        eprintln!("Commands will run without sandbox restrictions.");
        eprintln!("This is expected in non-sandbox mode (the default).");

        // Execute the remaining args directly without sandbox wrapping.
        // Format: cocode-linux-sandbox <sandbox-policy> <cwd> <command...>
        // For now, we skip the policy parsing and just run the command.
        let remaining_args: Vec<_> = args.collect();
        if remaining_args.len() >= 3 {
            // Args: [policy, cwd, command...]
            let cwd = &remaining_args[1];
            let command_args = &remaining_args[2..];

            if !command_args.is_empty() {
                use std::os::unix::process::CommandExt;
                let mut cmd = std::process::Command::new(&command_args[0]);
                cmd.args(&command_args[1..]);
                if let Some(cwd_str) = cwd.to_str() {
                    cmd.current_dir(cwd_str);
                }
                // This replaces the current process - never returns on success
                let err = cmd.exec();
                eprintln!("Failed to exec command: {err}");
                std::process::exit(1);
            }
        }

        // No command to execute or invalid args
        std::process::exit(0);
    }

    if exe_name == APPLY_PATCH_ARG0 || exe_name == MISSPELLED_APPLY_PATCH_ARG0 {
        // Dispatch to apply_patch CLI
        cocode_apply_patch::main();
    }

    // argv[1] hijack: --cocode-run-as-apply-patch
    let argv1 = args.next().unwrap_or_default();
    if argv1 == COCODE_APPLY_PATCH_ARG1 {
        let patch_arg = args.next().and_then(|s| s.to_str().map(str::to_owned));
        let exit_code = match patch_arg {
            Some(patch_arg) => {
                let mut stdout = std::io::stdout();
                let mut stderr = std::io::stderr();
                match cocode_apply_patch::apply_patch(&patch_arg, &mut stdout, &mut stderr) {
                    Ok(()) => 0,
                    Err(_) => 1,
                }
            }
            None => {
                eprintln!("Error: {COCODE_APPLY_PATCH_ARG1} requires a UTF-8 PATCH argument.");
                1
            }
        };
        std::process::exit(exit_code);
    }

    // Process hardening: remove dangerous env vars before any other
    // code runs. Defense-in-depth: sandbox also clears these per-command,
    // but cleaning at process level prevents accidental use in non-sandboxed
    // paths (e.g., MCP servers, subprocesses spawned before sandbox init).
    harden_process_env();

    // This modifies the environment, which is not thread-safe, so do this
    // before creating any threads/the Tokio runtime.
    load_dotenv();

    match prepend_path_entry_for_cocode_aliases() {
        Ok(path_entry) => Some(path_entry),
        Err(err) => {
            // It is possible that cocode will proceed successfully even if
            // updating the PATH fails, so warn the user and move on.
            eprintln!("WARNING: proceeding, even though we could not update PATH: {err}");
            None
        }
    }
}

/// Perform arg0 dispatch, then run the provided async main function.
///
/// This is the main entry point for binary crates that need arg0 dispatch.
/// It handles:
/// 1. arg0 dispatch for specialized CLIs
/// 2. Dotenv loading from ~/.cocode/.env
/// 3. PATH setup with symlinks for apply_patch
/// 4. Tokio runtime creation (16 MB worker stack)
/// 5. Running the provided async main function
///
/// The callback receives [`Arg0DispatchPaths`] containing helper executable
/// paths needed by downstream code.
pub fn arg0_dispatch_or_else<F, Fut>(main_fn: F) -> anyhow::Result<()>
where
    F: FnOnce(Arg0DispatchPaths) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    // Retain the guard so the temp dir and lock exist for the process lifetime.
    let path_entry = arg0_dispatch();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES)
        .build()?;
    runtime.block_on(async move {
        let current_exe = std::env::current_exe().ok();
        let paths = Arg0DispatchPaths {
            cocode_linux_sandbox_exe: if cfg!(target_os = "linux") {
                current_exe.or_else(|| {
                    path_entry
                        .as_ref()
                        .and_then(|g| g.paths().cocode_linux_sandbox_exe.clone())
                })
            } else {
                None
            },
        };

        main_fn(paths).await
    })
}

/// Remove environment variables that could be used for library injection attacks.
///
/// On Linux: `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT` can inject shared
/// libraries into the process. On macOS: `DYLD_INSERT_LIBRARIES`,
/// `DYLD_LIBRARY_PATH`, `DYLD_FRAMEWORK_PATH` serve the same purpose.
///
/// Called before any threads are spawned (single-threaded at this point).
fn harden_process_env() {
    let dangerous_vars = [
        // Linux library injection
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        // macOS library injection
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
    ];

    for var in &dangerous_vars {
        if std::env::var_os(var).is_some() {
            // SAFETY: This is called before any threads are spawned.
            unsafe { std::env::remove_var(var) };
            tracing::debug!("Process hardening: removed {var}");
        }
    }
}

/// Find the cocode home directory.
///
/// Returns `~/.cocode` or the value of `COCODE_HOME` if set.
fn find_cocode_home() -> std::io::Result<PathBuf> {
    // Check COCODE_HOME environment variable first
    if let Ok(home) = std::env::var("COCODE_HOME") {
        return Ok(PathBuf::from(home));
    }

    // Fall back to ~/.cocode
    dirs::home_dir().map(|h| h.join(".cocode")).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find home directory",
        )
    })
}

/// Load environment variables from ~/.cocode/.env.
///
/// Security: Do not allow `.env` files to create or modify any variables
/// with names starting with `COCODE_`.
fn load_dotenv() {
    if let Ok(cocode_home) = find_cocode_home() {
        let env_path = cocode_home.join(".env");
        if let Ok(iter) = dotenvy::from_path_iter(&env_path) {
            set_filtered(iter);
        }
    }
}

/// Helper to set vars from a dotenvy iterator while filtering out `COCODE_` keys.
fn set_filtered<I>(iter: I)
where
    I: IntoIterator<Item = Result<(String, String), dotenvy::Error>>,
{
    for (key, value) in iter.into_iter().flatten() {
        if !key.to_ascii_uppercase().starts_with(ILLEGAL_ENV_VAR_PREFIX) {
            // It is safe to call set_var() because our process is
            // single-threaded at this point in its execution.
            // SAFETY: This is called before any threads are spawned.
            unsafe { std::env::set_var(&key, &value) };
        }
    }
}

/// Creates a temporary directory with either:
///
/// - UNIX: `apply_patch` symlink to the current executable
/// - WINDOWS: `apply_patch.bat` batch script to invoke the current executable
///   with the "secret" --cocode-run-as-apply-patch flag.
///
/// This temporary directory is prepended to the PATH environment variable so
/// that `apply_patch` can be on the PATH without requiring the user to
/// install a separate `apply_patch` executable, simplifying the deployment of
/// cocode CLI.
///
/// IMPORTANT: This function modifies the PATH environment variable, so it MUST
/// be called before multiple threads are spawned.
pub fn prepend_path_entry_for_cocode_aliases() -> std::io::Result<Arg0PathEntryGuard> {
    let cocode_home = find_cocode_home()?;

    #[cfg(not(debug_assertions))]
    {
        // Guard against placing helpers in system temp directories outside debug builds.
        let temp_root = std::env::temp_dir();
        if cocode_home.starts_with(&temp_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Refusing to create helper binaries under temporary dir {temp_root:?} (cocode_home: {cocode_home:?})"
                ),
            ));
        }
    }

    std::fs::create_dir_all(&cocode_home)?;

    // Use a COCODE_HOME-scoped temp root to avoid cluttering the top-level directory.
    let temp_root = cocode_home.join("tmp").join("arg0");
    std::fs::create_dir_all(&temp_root)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Ensure only the current user can access the temp directory.
        std::fs::set_permissions(&temp_root, std::fs::Permissions::from_mode(0o700))?;
    }

    // Best-effort cleanup of stale per-session dirs. Ignore failures so startup proceeds.
    if let Err(err) = janitor_cleanup(&temp_root) {
        eprintln!("WARNING: failed to clean up stale arg0 temp dirs: {err}");
    }

    let temp_dir = tempfile::Builder::new()
        .prefix("cocode-arg0")
        .tempdir_in(&temp_root)?;
    let path = temp_dir.path();

    let lock_path = path.join(LOCK_FILENAME);
    let lock_file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.try_lock()?;

    for filename in &[
        APPLY_PATCH_ARG0,
        MISSPELLED_APPLY_PATCH_ARG0,
        #[cfg(target_os = "linux")]
        LINUX_SANDBOX_ARG0,
    ] {
        let exe = std::env::current_exe()?;

        #[cfg(unix)]
        {
            let link = path.join(filename);
            symlink(&exe, &link)?;
        }

        #[cfg(windows)]
        {
            let batch_script = path.join(format!("{filename}.bat"));
            std::fs::write(
                &batch_script,
                format!(
                    r#"@echo off
"{}" {COCODE_APPLY_PATCH_ARG1} %*
"#,
                    exe.display()
                ),
            )?;
        }
    }

    #[cfg(unix)]
    const PATH_SEPARATOR: &str = ":";

    #[cfg(windows)]
    const PATH_SEPARATOR: &str = ";";

    let path_element = path.display();
    let updated_path_env_var = match std::env::var("PATH") {
        Ok(existing_path) => {
            format!("{path_element}{PATH_SEPARATOR}{existing_path}")
        }
        Err(_) => {
            format!("{path_element}")
        }
    };

    // SAFETY: This is called before any threads are spawned.
    unsafe {
        std::env::set_var("PATH", updated_path_env_var);
    }

    let paths = Arg0DispatchPaths {
        cocode_linux_sandbox_exe: {
            #[cfg(target_os = "linux")]
            {
                Some(path.join(LINUX_SANDBOX_ARG0))
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        },
    };

    Ok(Arg0PathEntryGuard::new(temp_dir, lock_file, paths))
}

/// Remove stale per-session temp directories whose owning process has exited.
///
/// A directory is considered stale if its `.lock` file can be acquired (meaning
/// the process that created it is no longer running).
fn janitor_cleanup(temp_root: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(temp_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Skip the directory if locking fails or the lock is currently held.
        let Some(_lock_file) = try_lock_dir(&path)? else {
            continue;
        };

        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            // Expected TOCTOU race: directory can disappear after read_dir/lock checks.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

/// Try to acquire an exclusive lock on a directory's `.lock` file.
///
/// Returns `Ok(Some(file))` if acquired, `Ok(None)` if the lock is held or
/// there is no lock file, and `Err` on unexpected I/O errors.
fn try_lock_dir(dir: &Path) -> std::io::Result<Option<File>> {
    let lock_path = dir.join(LOCK_FILENAME);
    let lock_file = match File::options().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };

    match lock_file.try_lock() {
        Ok(()) => Ok(Some(lock_file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

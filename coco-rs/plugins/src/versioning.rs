//! Plugin version calculation per source type.
//!
//! TS source: `utils/plugins/pluginVersioning.ts:157`.
//!
//! Each source kind gets a deterministic version string used to compute the
//! versioned cache path `~/.coco/plugins/<name>/<version>/`.

use sha2::Digest;
use sha2::Sha256;

/// Source-aware identifier for version computation.
#[derive(Debug, Clone)]
pub enum VersionSource<'a> {
    /// Git source — use the short SHA when known, else `ref` name.
    Git {
        sha: Option<&'a str>,
        ref_: Option<&'a str>,
    },
    /// npm or pip — use the package version string.
    Package { version: &'a str },
    /// Local path or URL — content-hash the manifest payload.
    LocalOrUrl { manifest_bytes: &'a [u8] },
}

/// Compute the deterministic version string.
///
/// TS: `pluginVersioning.ts calculatePluginVersion(source, manifest)`.
pub fn calculate_plugin_version(source: VersionSource<'_>) -> String {
    match source {
        VersionSource::Git { sha: Some(s), .. } => short_sha(s).to_string(),
        VersionSource::Git {
            sha: None,
            ref_: Some(r),
        } => format!("ref-{r}"),
        VersionSource::Git {
            sha: None,
            ref_: None,
        } => "head".to_string(),
        VersionSource::Package { version } => version.to_string(),
        VersionSource::LocalOrUrl { manifest_bytes } => {
            let mut h = Sha256::new();
            h.update(manifest_bytes);
            let digest = h.finalize();
            // 12 hex chars is plenty for a content-addressed dir name.
            hex::encode(&digest[..6])
        }
    }
}

fn short_sha(sha: &str) -> &str {
    if sha.len() > 12 { &sha[..12] } else { sha }
}

/// Build the versioned cache path under a base dir.
pub fn versioned_cache_path(
    base: &std::path::Path,
    name: &str,
    version: &str,
) -> std::path::PathBuf {
    base.join(name).join(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_with_sha() {
        let v = calculate_plugin_version(VersionSource::Git {
            sha: Some("abcdef1234567890"),
            ref_: Some("main"),
        });
        assert_eq!(v, "abcdef123456");
    }

    #[test]
    fn git_with_only_ref() {
        let v = calculate_plugin_version(VersionSource::Git {
            sha: None,
            ref_: Some("v1"),
        });
        assert_eq!(v, "ref-v1");
    }

    #[test]
    fn package_version_passthrough() {
        let v = calculate_plugin_version(VersionSource::Package { version: "1.2.3" });
        assert_eq!(v, "1.2.3");
    }

    #[test]
    fn local_content_hash_stable() {
        let bytes = b"hello world";
        let v1 = calculate_plugin_version(VersionSource::LocalOrUrl {
            manifest_bytes: bytes,
        });
        let v2 = calculate_plugin_version(VersionSource::LocalOrUrl {
            manifest_bytes: bytes,
        });
        assert_eq!(v1, v2);
        assert_eq!(v1.len(), 12);
    }

    #[test]
    fn versioned_path_layout() {
        let p = versioned_cache_path(std::path::Path::new("/cache"), "foo", "1.0.0");
        assert_eq!(p, std::path::Path::new("/cache/foo/1.0.0"));
    }
}

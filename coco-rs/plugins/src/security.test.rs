use super::*;

#[test]
fn validate_paths_accepts_normal() {
    assert_eq!(validate_paths("foo/bar.md"), PathValidation::Ok);
    assert_eq!(validate_paths(""), PathValidation::Ok);
}

#[test]
fn validate_paths_rejects_absolute() {
    assert_eq!(validate_paths("/etc/passwd"), PathValidation::Absolute);
}

#[test]
fn validate_paths_rejects_dotdot() {
    assert_eq!(validate_paths("../escape"), PathValidation::DotDotSegment);
    assert_eq!(validate_paths("a/../b"), PathValidation::DotDotSegment);
}

#[test]
fn check_impersonation_blocks_official_name() {
    let r = check_impersonation("claude-plugins-official", false);
    assert!(matches!(r, ImpersonationResult::OfficialNameMatch { .. }));
}

#[test]
fn check_impersonation_blocks_variants() {
    for name in [
        "claude-plugin-official",
        "ClaudePluginsOfficial",
        "claude_plugins_official",
        "claude-plugins-official-extras",
        "anthropic-plugins",
        "claude-code-official",
    ] {
        let r = check_impersonation(name, false);
        assert!(
            matches!(r, ImpersonationResult::OfficialNameMatch { .. }),
            "should block {name}: got {r:?}"
        );
    }
}

#[test]
fn check_impersonation_allows_clean_names() {
    let r = check_impersonation("my-cool-plugin", false);
    assert_eq!(r, ImpersonationResult::Ok);
}

#[test]
fn check_impersonation_allows_official_marketplace() {
    let r = check_impersonation("claude-plugins-official", true);
    assert_eq!(r, ImpersonationResult::Ok);
}

#[test]
fn check_impersonation_catches_cyrillic_homograph() {
    // 'а' is Cyrillic small a (U+0430), which folds to ASCII 'a'.
    let r = check_impersonation("clаude-plugins-official", false);
    assert!(matches!(r, ImpersonationResult::HomographMatch { .. }));
}

#[test]
fn policy_blocks_marketplace_explicitly() {
    let p = EnterprisePolicy {
        blocked_marketplaces: vec!["evil".into()],
        ..Default::default()
    };
    let id = PluginId::parse("foo@evil");
    assert_eq!(
        check_policy(&id, false, &p),
        PolicyVerdict::BlockedMarketplace {
            marketplace: "evil".into()
        }
    );
}

#[test]
fn policy_strict_known_blocks_unlisted() {
    let p = EnterprisePolicy {
        strict_known_marketplaces: true,
        known_marketplaces: vec!["approved".into()],
        ..Default::default()
    };
    let unlisted = PluginId::parse("foo@unknown");
    assert!(matches!(
        check_policy(&unlisted, false, &p),
        PolicyVerdict::UnapprovedMarketplace { .. }
    ));
    let approved = PluginId::parse("foo@approved");
    assert_eq!(check_policy(&approved, false, &p), PolicyVerdict::Ok);
}

#[test]
fn policy_strict_user_scope_forbids_user_install() {
    let p = EnterprisePolicy {
        strict_plugin_only_customization: true,
        ..Default::default()
    };
    let id = PluginId::parse("foo@m");
    assert_eq!(
        check_policy(&id, true, &p),
        PolicyVerdict::UserScopeForbidden
    );
    assert_eq!(check_policy(&id, false, &p), PolicyVerdict::Ok);
}

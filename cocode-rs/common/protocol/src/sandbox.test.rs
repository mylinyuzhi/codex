use super::*;

#[test]
fn test_default() {
    assert_eq!(SandboxMode::default(), SandboxMode::ReadOnly);
}

#[test]
fn test_allows_write() {
    assert!(!SandboxMode::ReadOnly.allows_write());
    assert!(SandboxMode::WorkspaceWrite.allows_write());
    assert!(SandboxMode::FullAccess.allows_write());
    assert!(SandboxMode::ExternalSandbox.allows_write());
}

#[test]
fn test_is_full_access() {
    assert!(!SandboxMode::ReadOnly.is_full_access());
    assert!(!SandboxMode::WorkspaceWrite.is_full_access());
    assert!(SandboxMode::FullAccess.is_full_access());
    assert!(!SandboxMode::ExternalSandbox.is_full_access());
}

#[test]
fn test_is_external_sandbox() {
    assert!(!SandboxMode::ReadOnly.is_external_sandbox());
    assert!(!SandboxMode::WorkspaceWrite.is_external_sandbox());
    assert!(!SandboxMode::FullAccess.is_external_sandbox());
    assert!(SandboxMode::ExternalSandbox.is_external_sandbox());
}

#[test]
fn test_as_str() {
    assert_eq!(SandboxMode::ReadOnly.as_str(), "read-only");
    assert_eq!(SandboxMode::WorkspaceWrite.as_str(), "workspace-write");
    assert_eq!(SandboxMode::FullAccess.as_str(), "full-access");
    assert_eq!(SandboxMode::ExternalSandbox.as_str(), "external-sandbox");
}

#[test]
fn test_display() {
    assert_eq!(format!("{}", SandboxMode::ReadOnly), "read-only");
    assert_eq!(
        format!("{}", SandboxMode::WorkspaceWrite),
        "workspace-write"
    );
    assert_eq!(format!("{}", SandboxMode::FullAccess), "full-access");
    assert_eq!(
        format!("{}", SandboxMode::ExternalSandbox),
        "external-sandbox"
    );
}

#[test]
fn test_from_str() {
    // Primary format
    assert_eq!(
        "read-only".parse::<SandboxMode>().unwrap(),
        SandboxMode::ReadOnly
    );
    assert_eq!(
        "workspace-write".parse::<SandboxMode>().unwrap(),
        SandboxMode::WorkspaceWrite
    );
    assert_eq!(
        "full-access".parse::<SandboxMode>().unwrap(),
        SandboxMode::FullAccess
    );

    assert_eq!(
        "external-sandbox".parse::<SandboxMode>().unwrap(),
        SandboxMode::ExternalSandbox
    );

    // Alternative formats
    assert_eq!(
        "readonly".parse::<SandboxMode>().unwrap(),
        SandboxMode::ReadOnly
    );
    assert_eq!(
        "read_only".parse::<SandboxMode>().unwrap(),
        SandboxMode::ReadOnly
    );
    assert_eq!(
        "external".parse::<SandboxMode>().unwrap(),
        SandboxMode::ExternalSandbox
    );
    assert_eq!(
        "external_sandbox".parse::<SandboxMode>().unwrap(),
        SandboxMode::ExternalSandbox
    );
}

#[test]
fn test_from_str_error() {
    let result = "invalid".parse::<SandboxMode>();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown sandbox mode"));
}

#[test]
fn test_serde_roundtrip() {
    for mode in [
        SandboxMode::ReadOnly,
        SandboxMode::WorkspaceWrite,
        SandboxMode::FullAccess,
        SandboxMode::ExternalSandbox,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: SandboxMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mode);
    }
}

#[test]
fn test_serde_kebab_case() {
    // Verify kebab-case serialization
    assert_eq!(
        serde_json::to_string(&SandboxMode::ReadOnly).unwrap(),
        "\"read-only\""
    );
    assert_eq!(
        serde_json::to_string(&SandboxMode::WorkspaceWrite).unwrap(),
        "\"workspace-write\""
    );
    assert_eq!(
        serde_json::to_string(&SandboxMode::FullAccess).unwrap(),
        "\"full-access\""
    );
    assert_eq!(
        serde_json::to_string(&SandboxMode::ExternalSandbox).unwrap(),
        "\"external-sandbox\""
    );
}

#[test]
fn test_serde_deserialize() {
    assert_eq!(
        serde_json::from_str::<SandboxMode>("\"read-only\"").unwrap(),
        SandboxMode::ReadOnly
    );
    assert_eq!(
        serde_json::from_str::<SandboxMode>("\"workspace-write\"").unwrap(),
        SandboxMode::WorkspaceWrite
    );
    assert_eq!(
        serde_json::from_str::<SandboxMode>("\"full-access\"").unwrap(),
        SandboxMode::FullAccess
    );
    assert_eq!(
        serde_json::from_str::<SandboxMode>("\"external-sandbox\"").unwrap(),
        SandboxMode::ExternalSandbox
    );
}

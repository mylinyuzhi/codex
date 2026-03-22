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
}

#[test]
fn test_is_full_access() {
    assert!(!SandboxMode::ReadOnly.is_full_access());
    assert!(!SandboxMode::WorkspaceWrite.is_full_access());
    assert!(SandboxMode::FullAccess.is_full_access());
}

#[test]
fn test_as_str() {
    assert_eq!(SandboxMode::ReadOnly.as_str(), "read-only");
    assert_eq!(SandboxMode::WorkspaceWrite.as_str(), "workspace-write");
    assert_eq!(SandboxMode::FullAccess.as_str(), "full-access");
}

#[test]
fn test_display() {
    assert_eq!(format!("{}", SandboxMode::ReadOnly), "read-only");
    assert_eq!(
        format!("{}", SandboxMode::WorkspaceWrite),
        "workspace-write"
    );
    assert_eq!(format!("{}", SandboxMode::FullAccess), "full-access");
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

    // Alternative formats
    assert_eq!(
        "readonly".parse::<SandboxMode>().unwrap(),
        SandboxMode::ReadOnly
    );
    assert_eq!(
        "read_only".parse::<SandboxMode>().unwrap(),
        SandboxMode::ReadOnly
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
}

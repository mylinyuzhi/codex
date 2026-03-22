use cocode_error::ErrorExt;
use cocode_error::StatusCode;

use crate::error::TeamError;
use crate::error::team_error;

#[test]
fn team_not_found_status_code() {
    let err: TeamError = team_error::TeamNotFoundSnafu { name: "test-team" }.build();
    assert_eq!(err.status_code(), StatusCode::FileNotFound);
    assert!(err.to_string().contains("test-team"));
}

#[test]
fn team_exists_status_code() {
    let err: TeamError = team_error::TeamExistsSnafu { name: "test-team" }.build();
    assert_eq!(err.status_code(), StatusCode::InvalidArguments);
}

#[test]
fn not_a_member_status_code() {
    let err: TeamError = team_error::NotAMemberSnafu {
        agent_id: "a123",
        team_name: "test-team",
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::FileNotFound);
    assert!(err.to_string().contains("a123"));
    assert!(err.to_string().contains("test-team"));
}

#[test]
fn max_members_status_code() {
    let err: TeamError = team_error::MaxMembersReachedSnafu {
        team_name: "test-team",
        limit: 10_usize,
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::InvalidArguments);
}

#[test]
fn shutdown_timeout_status_code() {
    let err: TeamError = team_error::ShutdownTimeoutSnafu { agent_id: "a123" }.build();
    assert_eq!(err.status_code(), StatusCode::Timeout);
}

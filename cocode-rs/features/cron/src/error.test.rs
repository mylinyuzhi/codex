use cocode_error::ErrorExt;
use cocode_error::StatusCode;

use super::*;

#[test]
fn test_invalid_schedule_status_code() {
    let err = CronError::InvalidSchedule {
        message: "bad cron".to_string(),
        location: cocode_error::Location::default(),
    };
    assert_eq!(err.status_code(), StatusCode::InvalidArguments);
}

#[test]
fn test_job_not_found_status_code() {
    let err = CronError::JobNotFound {
        id: "cron_abc".to_string(),
        location: cocode_error::Location::default(),
    };
    assert_eq!(err.status_code(), StatusCode::FileNotFound);
}

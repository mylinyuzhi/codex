#[test]
fn test_ssh_to_https_conversion() {
    let ssh_url = "git@github.com:owner/repo.git";
    let https = ssh_url
        .replace("git@github.com:", "https://github.com/")
        .trim_end_matches(".git")
        .to_string()
        + ".git";
    assert_eq!(https, "https://github.com/owner/repo.git");
}

#[test]
fn test_ssh_fallback_detection() {
    assert!("git@github.com:owner/repo.git".starts_with("git@github.com:"));
    assert!(!"https://github.com/owner/repo.git".starts_with("git@github.com:"));
}

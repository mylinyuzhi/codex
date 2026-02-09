use super::*;
use anyhow::Result;
use keyring::Error as KeyringError;
use pretty_assertions::assert_eq;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::sync::PoisonError;
use tempfile::tempdir;

use cocode_keyring_store::tests::MockKeyringStore;

struct TempCodexHome {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

impl TempCodexHome {
    fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let dir = tempdir().expect("create CODEX_HOME temp dir");
        unsafe {
            std::env::set_var("CODEX_HOME", dir.path());
        }
        Self {
            _guard: guard,
            _dir: dir,
        }
    }
}

impl Drop for TempCodexHome {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("CODEX_HOME");
        }
    }
}

#[test]
fn load_oauth_tokens_reads_from_keyring_when_available() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let expected = tokens.clone();
    let serialized = serde_json::to_string(&tokens)?;
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    store.save(KEYRING_SERVICE, &key, &serialized)?;

    let loaded =
        super::load_oauth_tokens_from_keyring(&store, &tokens.server_name, &tokens.url)?
            .expect("tokens should load from keyring");
    assert_tokens_match_without_expiry(&loaded, &expected);
    Ok(())
}

#[test]
fn load_oauth_tokens_falls_back_when_missing_in_keyring() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let expected = tokens.clone();

    super::save_oauth_tokens_to_file(&tokens)?;

    let loaded = super::load_oauth_tokens_from_keyring_with_fallback_to_file(
        &store,
        &tokens.server_name,
        &tokens.url,
    )?
    .expect("tokens should load from fallback");
    assert_tokens_match_without_expiry(&loaded, &expected);
    Ok(())
}

#[test]
fn load_oauth_tokens_falls_back_when_keyring_errors() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let expected = tokens.clone();
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    store.set_error(&key, KeyringError::Invalid("error".into(), "load".into()));

    super::save_oauth_tokens_to_file(&tokens)?;

    let loaded = super::load_oauth_tokens_from_keyring_with_fallback_to_file(
        &store,
        &tokens.server_name,
        &tokens.url,
    )?
    .expect("tokens should load from fallback");
    assert_tokens_match_without_expiry(&loaded, &expected);
    Ok(())
}

#[test]
fn save_oauth_tokens_prefers_keyring_when_available() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;

    super::save_oauth_tokens_to_file(&tokens)?;

    super::save_oauth_tokens_with_keyring_with_fallback_to_file(
        &store,
        &tokens.server_name,
        &tokens,
    )?;

    let fallback_path = super::fallback_file_path()?;
    assert!(!fallback_path.exists(), "fallback file should be removed");
    let stored = store.saved_value(&key).expect("value saved to keyring");
    assert_eq!(serde_json::from_str::<StoredOAuthTokens>(&stored)?, tokens);
    Ok(())
}

#[test]
fn save_oauth_tokens_writes_fallback_when_keyring_fails() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    store.set_error(&key, KeyringError::Invalid("error".into(), "save".into()));

    super::save_oauth_tokens_with_keyring_with_fallback_to_file(
        &store,
        &tokens.server_name,
        &tokens,
    )?;

    let fallback_path = super::fallback_file_path()?;
    assert!(fallback_path.exists(), "fallback file should be created");
    let saved = super::read_fallback_file()?.expect("fallback file should load");
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    let entry = saved.get(&key).expect("entry for key");
    assert_eq!(entry.server_name, tokens.server_name);
    assert_eq!(entry.server_url, tokens.url);
    assert_eq!(entry.client_id, tokens.client_id);
    assert_eq!(
        entry.access_token,
        tokens.token_response.0.access_token().secret().as_str()
    );
    assert!(store.saved_value(&key).is_none());
    Ok(())
}

#[test]
fn delete_oauth_tokens_removes_all_storage() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let serialized = serde_json::to_string(&tokens)?;
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    store.save(KEYRING_SERVICE, &key, &serialized)?;
    super::save_oauth_tokens_to_file(&tokens)?;

    let removed = super::delete_oauth_tokens_from_keyring_and_file(
        &store,
        OAuthCredentialsStoreMode::Auto,
        &tokens.server_name,
        &tokens.url,
    )?;
    assert!(removed);
    assert!(!store.contains(&key));
    assert!(!super::fallback_file_path()?.exists());
    Ok(())
}

#[test]
fn delete_oauth_tokens_file_mode_removes_keyring_only_entry() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let serialized = serde_json::to_string(&tokens)?;
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    store.save(KEYRING_SERVICE, &key, &serialized)?;
    assert!(store.contains(&key));

    let removed = super::delete_oauth_tokens_from_keyring_and_file(
        &store,
        OAuthCredentialsStoreMode::Auto,
        &tokens.server_name,
        &tokens.url,
    )?;
    assert!(removed);
    assert!(!store.contains(&key));
    assert!(!super::fallback_file_path()?.exists());
    Ok(())
}

#[test]
fn delete_oauth_tokens_propagates_keyring_errors() -> Result<()> {
    let _env = TempCodexHome::new();
    let store = MockKeyringStore::default();
    let tokens = sample_tokens();
    let key = super::compute_store_key(&tokens.server_name, &tokens.url)?;
    store.set_error(&key, KeyringError::Invalid("error".into(), "delete".into()));
    super::save_oauth_tokens_to_file(&tokens).unwrap();

    let result = super::delete_oauth_tokens_from_keyring_and_file(
        &store,
        OAuthCredentialsStoreMode::Auto,
        &tokens.server_name,
        &tokens.url,
    );
    assert!(result.is_err());
    assert!(super::fallback_file_path().unwrap().exists());
    Ok(())
}

#[test]
fn refresh_expires_in_from_timestamp_restores_future_durations() {
    let mut tokens = sample_tokens();
    let expires_at = tokens.expires_at.expect("expires_at should be set");

    tokens.token_response.0.set_expires_in(None);
    super::refresh_expires_in_from_timestamp(&mut tokens);

    let actual = tokens
        .token_response
        .0
        .expires_in()
        .expect("expires_in should be restored")
        .as_secs();
    let expected = super::expires_in_from_timestamp(expires_at)
        .expect("expires_at should still be in the future");
    let diff = actual.abs_diff(expected);
    assert!(diff <= 1, "expires_in drift too large: diff={diff}");
}

#[test]
fn refresh_expires_in_from_timestamp_clears_expired_tokens() {
    let mut tokens = sample_tokens();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let expired_at = now.as_millis() as u64;
    tokens.expires_at = Some(expired_at.saturating_sub(1000));

    let duration = Duration::from_secs(600);
    tokens.token_response.0.set_expires_in(Some(&duration));

    super::refresh_expires_in_from_timestamp(&mut tokens);

    assert!(tokens.token_response.0.expires_in().is_none());
}

fn assert_tokens_match_without_expiry(
    actual: &StoredOAuthTokens,
    expected: &StoredOAuthTokens,
) {
    assert_eq!(actual.server_name, expected.server_name);
    assert_eq!(actual.url, expected.url);
    assert_eq!(actual.client_id, expected.client_id);
    assert_eq!(actual.expires_at, expected.expires_at);
    assert_token_response_match_without_expiry(
        &actual.token_response,
        &expected.token_response,
    );
}

fn assert_token_response_match_without_expiry(
    actual: &WrappedOAuthTokenResponse,
    expected: &WrappedOAuthTokenResponse,
) {
    let actual_response = &actual.0;
    let expected_response = &expected.0;

    assert_eq!(
        actual_response.access_token().secret(),
        expected_response.access_token().secret()
    );
    assert_eq!(actual_response.token_type(), expected_response.token_type());
    assert_eq!(
        actual_response.refresh_token().map(RefreshToken::secret),
        expected_response.refresh_token().map(RefreshToken::secret),
    );
    assert_eq!(actual_response.scopes(), expected_response.scopes());
    assert_eq!(
        actual_response.extra_fields(),
        expected_response.extra_fields()
    );
    assert_eq!(
        actual_response.expires_in().is_some(),
        expected_response.expires_in().is_some()
    );
}

fn sample_tokens() -> StoredOAuthTokens {
    let mut response = OAuthTokenResponse::new(
        AccessToken::new("access-token".to_string()),
        BasicTokenType::Bearer,
        EmptyExtraTokenFields {},
    );
    response.set_refresh_token(Some(RefreshToken::new("refresh-token".to_string())));
    response.set_scopes(Some(vec![
        Scope::new("scope-a".to_string()),
        Scope::new("scope-b".to_string()),
    ]));
    let expires_in = Duration::from_secs(3600);
    response.set_expires_in(Some(&expires_in));
    let expires_at = super::compute_expires_at_millis(&response);

    StoredOAuthTokens {
        server_name: "test-server".to_string(),
        url: "https://example.test".to_string(),
        client_id: "client-id".to_string(),
        token_response: WrappedOAuthTokenResponse(response),
        expires_at,
    }
}

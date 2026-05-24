use super::*;

#[test]
fn test_jwt_cache_creation_valid_key() {
    let cache = JwtTokenCache::new("api_key_id.secret");
    assert!(cache.is_ok());
}

#[test]
fn test_jwt_cache_creation_invalid_key() {
    let cache = JwtTokenCache::new("invalid_key");
    assert!(cache.is_err());
}

#[tokio::test]
async fn test_jwt_token_generation() {
    let cache = JwtTokenCache::new("test_id.test_secret").expect("valid key");
    let token = cache.get_token().await;
    assert!(token.is_ok());

    let token = token.expect("token");
    // JWT has 3 parts separated by dots
    assert_eq!(token.split('.').count(), 3);
}

#[tokio::test]
async fn test_jwt_token_caching() {
    let cache = JwtTokenCache::new("test_id.test_secret").expect("valid key");

    let token1 = cache.get_token().await.expect("token1");
    let token2 = cache.get_token().await.expect("token2");

    // Tokens should be the same (cached)
    assert_eq!(token1, token2);
}

use streamx::server::auth;

#[test]
fn bcrypt_hash_and_verify() {
    let password = "my_secret_password";
    let hash = auth::hash_password(password).unwrap();

    assert_ne!(hash, password);
    assert!(bcrypt::verify(password, &hash).unwrap());
}

#[test]
fn bcrypt_verify_wrong_password_fails() {
    let hash = auth::hash_password("correct_password").unwrap();
    assert!(!bcrypt::verify("wrong_password", &hash).unwrap());
}

#[test]
fn jwt_create_and_validate() {
    let secret = "test-jwt-secret";
    let token = auth::create_jwt("user-123", "testuser", false, secret, 24).unwrap();

    let claims = auth::validate_jwt(&token, secret).unwrap();
    assert_eq!(claims.user_id, "user-123");
    assert_eq!(claims.username, "testuser");
}

#[test]
fn jwt_with_wrong_secret_fails() {
    let token = auth::create_jwt("user-123", "testuser", false, "secret-a", 24).unwrap();
    let result = auth::validate_jwt(&token, "secret-b");
    assert!(result.is_err());
}

#[test]
fn jwt_with_expired_token_fails() {
    let secret = "test-jwt-secret";
    let token = auth::create_jwt("user-123", "testuser", false, secret, -1).unwrap();

    let result = auth::validate_jwt(&token, secret);
    assert!(result.is_err());
}

#[tokio::test]
async fn rate_limiter_allows_under_limit() {
    let limiter = auth::RateLimiter::new();
    let ip = "192.168.1.1";

    for _ in 0..10 {
        let result = limiter.check(ip).await;
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn rate_limiter_blocks_over_limit() {
    let limiter = auth::RateLimiter::new();
    let ip = "10.0.0.1";

    for _ in 0..10 {
        limiter.check(ip).await.unwrap();
    }

    let result = limiter.check(ip).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn rate_limiter_tracks_ips_independently() {
    let limiter = auth::RateLimiter::new();

    for _ in 0..10 {
        limiter.check("1.1.1.1").await.unwrap();
    }

    let result_blocked = limiter.check("1.1.1.1").await;
    assert!(result_blocked.is_err());

    let result_other = limiter.check("2.2.2.2").await;
    assert!(result_other.is_ok());
}

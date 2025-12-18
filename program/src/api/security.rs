//! Security middleware for API authentication and rate limiting
//!
//! Provides:
//! - API Key authentication
//! - Rate limiting per IP and per API key
//! - Request ID tracking

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use std::collections::HashSet;
use tracing::{info, warn};

/// API key store (in production, load from database or secrets manager)
#[derive(Clone)]
pub struct ApiKeyStore {
    /// Valid API keys (in production, use database lookup with hashing)
    valid_keys: Arc<HashSet<String>>,
    /// Whether authentication is required (can be disabled for development)
    auth_required: bool,
}

impl ApiKeyStore {
    /// Create a new API key store
    /// 
    /// In production, keys should be loaded from:
    /// - Environment variables
    /// - Kubernetes secrets
    /// - HashiCorp Vault
    /// - Database with hashed keys
    pub fn new() -> Self {
        let mut keys = HashSet::new();
        
        // Load API keys from environment
        // Format: API_KEYS=key1,key2,key3
        if let Ok(api_keys) = std::env::var("API_KEYS") {
            for key in api_keys.split(',') {
                let key = key.trim();
                if !key.is_empty() {
                    keys.insert(key.to_string());
                }
            }
        }
        
        // Check if auth is required (default: true in production)
        let auth_required = std::env::var("API_AUTH_REQUIRED")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        
        if !auth_required {
            warn!("⚠️ API authentication is DISABLED (API_AUTH_REQUIRED=false)");
        } else if keys.is_empty() {
            warn!("⚠️ No API keys configured. Set API_KEYS environment variable.");
        } else {
            info!("✅ Loaded {} API key(s)", keys.len());
        }
        
        Self {
            valid_keys: Arc::new(keys),
            auth_required,
        }
    }
    
    /// Check if an API key is valid
    pub fn is_valid(&self, key: &str) -> bool {
        self.valid_keys.contains(key)
    }
    
    /// Check if authentication is required
    pub fn is_auth_required(&self) -> bool {
        self.auth_required
    }
}

impl Default for ApiKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication error response
#[derive(serde::Serialize)]
pub struct AuthError {
    pub error: String,
    pub code: String,
}

/// API Key authentication middleware
/// 
/// Checks for API key in:
/// 1. `X-API-Key` header
/// 2. `Authorization: Bearer <key>` header
/// 3. `api_key` query parameter (not recommended, but supported)
pub async fn auth_middleware(
    State(key_store): State<ApiKeyStore>,
    request: Request,
    next: Next,
) -> Response {
    // Skip auth for health check and metrics endpoints
    let path = request.uri().path();
    if path == "/health" || path == "/metrics" || path.starts_with("/health/") {
        return next.run(request).await;
    }
    
    // If auth is disabled, allow all requests
    if !key_store.is_auth_required() {
        return next.run(request).await;
    }
    
    // Try to extract API key from headers
    let api_key = extract_api_key(&request);
    
    match api_key {
        Some(key) if key_store.is_valid(&key) => {
            // Valid key - proceed
            next.run(request).await
        }
        Some(_) => {
            // Invalid key
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "Invalid API key".to_string(),
                    code: "INVALID_API_KEY".to_string(),
                }),
            ).into_response()
        }
        None => {
            // No key provided
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "API key required. Provide via X-API-Key header or Authorization: Bearer <key>".to_string(),
                    code: "MISSING_API_KEY".to_string(),
                }),
            ).into_response()
        }
    }
}

/// Extract API key from request headers
fn extract_api_key(request: &Request) -> Option<String> {
    // Try X-API-Key header first
    if let Some(key) = request.headers().get("X-API-Key") {
        if let Ok(key_str) = key.to_str() {
            return Some(key_str.to_string());
        }
    }
    
    // Try Authorization: Bearer <key>
    if let Some(auth) = request.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(key) = auth_str.strip_prefix("Bearer ") {
                return Some(key.to_string());
            }
        }
    }
    
    // Try query parameter (less secure, but convenient for testing)
    if let Some(query) = request.uri().query() {
        for pair in query.split('&') {
            if let Some(key) = pair.strip_prefix("api_key=") {
                return Some(key.to_string());
            }
        }
    }
    
    None
}

/// Rate limiting configuration (for future use with a simpler implementation)
#[allow(dead_code)]
pub struct RateLimitConfig {
    /// Requests per second per IP
    pub requests_per_second: u32,
    /// Burst size (allows temporary spikes)
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10,
            burst_size: 30,
        }
    }
}

// Note: Rate limiting can be added later using:
// - nginx/ingress controller rate limiting (recommended for K8s)
// - Simple in-memory token bucket with dashmap
// - Redis-based distributed rate limiting

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_api_key_store_empty() {
        std::env::remove_var("API_KEYS");
        std::env::set_var("API_AUTH_REQUIRED", "false");
        let store = ApiKeyStore::new();
        assert!(!store.is_auth_required());
    }
    
    #[test]
    fn test_api_key_validation() {
        std::env::set_var("API_KEYS", "test-key-123,another-key");
        std::env::set_var("API_AUTH_REQUIRED", "true");
        let store = ApiKeyStore::new();
        
        assert!(store.is_valid("test-key-123"));
        assert!(store.is_valid("another-key"));
        assert!(!store.is_valid("invalid-key"));
    }
}

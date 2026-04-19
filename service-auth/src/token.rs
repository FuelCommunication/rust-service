use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AuthError, AuthResult};

const ISSUER: &str = "service-auth";
const TOKEN_TYPE_ACCESS: &str = "access";
const TOKEN_TYPE_REFRESH: &str = "refresh";

#[derive(Debug, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: String,
    pub email: String,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
    pub jti: String,
    pub iss: String,
    pub typ: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshClaims {
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
    pub jti: String,
    pub iss: String,
    pub typ: String,
}

pub struct TokenManager {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    access_expiration_secs: u64,
    refresh_expiration_secs: u64,
    header: Header,
    access_validation: Validation,
    refresh_validation: Validation,
}

impl TokenManager {
    pub fn new(secret: &str, access_expiration_secs: u64, refresh_expiration_secs: u64) -> Self {
        let mut access_validation = Validation::new(Algorithm::HS256);
        access_validation.leeway = 5;
        access_validation.set_issuer(&[ISSUER]);
        access_validation.set_required_spec_claims(&["exp", "iss", "sub"]);

        let mut refresh_validation = Validation::new(Algorithm::HS256);
        refresh_validation.leeway = 5;
        refresh_validation.set_issuer(&[ISSUER]);
        refresh_validation.set_required_spec_claims(&["exp", "iss", "sub"]);

        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            access_expiration_secs,
            refresh_expiration_secs,
            header: Header::new(Algorithm::HS256),
            access_validation,
            refresh_validation,
        }
    }

    pub fn create_access_token(&self, user_id: Uuid, email: &str, username: &str) -> AuthResult<(String, i64)> {
        let now = chrono::Utc::now().timestamp() as usize;
        let exp = now + self.access_expiration_secs as usize;

        let claims = AccessClaims {
            sub: user_id.to_string(),
            email: email.to_owned(),
            username: username.to_owned(),
            exp,
            iat: now,
            jti: Uuid::now_v7().to_string(),
            iss: ISSUER.to_owned(),
            typ: TOKEN_TYPE_ACCESS.to_owned(),
        };

        let token = encode(&self.header, &claims, &self.encoding_key).map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok((token, exp as i64))
    }

    pub fn create_refresh_token(&self, user_id: Uuid) -> AuthResult<(String, Uuid, i64)> {
        let now = chrono::Utc::now().timestamp() as usize;
        let exp = now + self.refresh_expiration_secs as usize;
        let jti = Uuid::now_v7();

        let claims = RefreshClaims {
            sub: user_id.to_string(),
            exp,
            iat: now,
            jti: jti.to_string(),
            iss: ISSUER.to_owned(),
            typ: TOKEN_TYPE_REFRESH.to_owned(),
        };
        let token = encode(&self.header, &claims, &self.encoding_key).map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok((token, jti, exp as i64))
    }

    pub fn validate_access_token(&self, token: &str) -> AuthResult<AccessClaims> {
        let claims: AccessClaims = decode(token, &self.decoding_key, &self.access_validation)
            .map(|data| data.claims)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::InvalidToken(e.to_string()),
            })?;

        if claims.typ != TOKEN_TYPE_ACCESS {
            return Err(AuthError::InvalidToken("invalid token type".into()));
        }

        Ok(claims)
    }

    pub fn validate_refresh_token(&self, token: &str) -> AuthResult<RefreshClaims> {
        let claims: RefreshClaims = decode(token, &self.decoding_key, &self.refresh_validation)
            .map(|data| data.claims)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::InvalidToken(e.to_string()),
            })?;

        if claims.typ != TOKEN_TYPE_REFRESH {
            return Err(AuthError::InvalidToken("invalid token type".into()));
        }

        Ok(claims)
    }
}

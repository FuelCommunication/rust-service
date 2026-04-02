use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use prost_types::Timestamp;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::error::AuthError;
use crate::proto::{
    AuthTokens, LoginRequest, LoginResponse, LogoutRequest, OAuthAuthenticateRequest, OAuthGetAuthUrlRequest,
    OAuthGetAuthUrlResponse, OAuthProvider, RefreshTokenRequest, RegisterRequest, RegisterResponse, ValidateTokenRequest,
    ValidateTokenResponse, auth_service_server::AuthService,
};
use crate::state::ServerState;

const MAX_PASSWORD_LENGTH: usize = 128;
const MAX_USERNAME_LENGTH: usize = 64;
const MIN_USERNAME_LENGTH: usize = 2;

fn to_timestamp(secs: i64) -> Option<Timestamp> {
    Some(Timestamp { seconds: secs, nanos: 0 })
}

fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.splitn(2, '@').collect();
    parts.len() == 2 && !parts[0].is_empty() && parts[1].contains('.') && parts[1].len() >= 3
}

fn is_valid_username(username: &str) -> bool {
    username.len() >= MIN_USERNAME_LENGTH && username.len() <= MAX_USERNAME_LENGTH
}

pub struct AuthServiceImpl {
    state: ServerState,
}

impl AuthServiceImpl {
    pub fn new(state: ServerState) -> Self {
        Self { state }
    }

    async fn hash_password(password: String) -> Result<String, AuthError> {
        tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|h| h.to_string())
                .map_err(|e| AuthError::HashingError(e.to_string()))
        })
        .await
        .map_err(|e| AuthError::Internal(format!("hash task failed: {e}")))?
    }

    async fn verify_password(password: String, hash: String) -> Result<(), AuthError> {
        tokio::task::spawn_blocking(move || {
            let parsed = PasswordHash::new(&hash).map_err(|e| AuthError::HashingError(e.to_string()))?;
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .map_err(|_| AuthError::InvalidCredentials)
        })
        .await
        .map_err(|e| AuthError::Internal(format!("verify task failed: {e}")))?
    }

    async fn dummy_verify() {
        let _ = tokio::task::spawn_blocking(|| {
            let dummy_hash = "$argon2id$v=19$m=19456,t=2,p=1$dummysaltdummysa$aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
            let parsed = PasswordHash::new(dummy_hash).ok();
            if let Some(parsed) = parsed {
                let _ = Argon2::default().verify_password(b"dummy", &parsed);
            }
        })
        .await;
    }

    async fn issue_tokens(&self, user_id: Uuid, email: &str, username: &str) -> Result<AuthTokens, AuthError> {
        let (access_token, access_expires_at) = self.state.tokens.create_access_token(user_id, email, username)?;
        let (refresh_token, jti, refresh_expires_at) = self.state.tokens.create_refresh_token(user_id)?;

        let expires_at = chrono::DateTime::from_timestamp(refresh_expires_at, 0)
            .ok_or_else(|| AuthError::Internal("invalid token expiration timestamp".into()))?;
        self.state.store.store_refresh_token(jti, user_id, expires_at).await?;

        Ok(AuthTokens {
            access_token,
            refresh_token,
            access_expires_at: to_timestamp(access_expires_at),
            refresh_expires_at: to_timestamp(refresh_expires_at),
        })
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    #[tracing::instrument(skip_all, fields(email = %req.get_ref().email))]
    async fn register(&self, req: Request<RegisterRequest>) -> Result<Response<RegisterResponse>, Status> {
        let req = req.into_inner();

        if req.email.is_empty() || req.password.is_empty() || req.username.is_empty() {
            return Err(AuthError::InvalidArgument("email, password and username are required".into()).into());
        }

        if !is_valid_email(&req.email) {
            return Err(AuthError::InvalidArgument("invalid email format".into()).into());
        }

        if !is_valid_username(&req.username) {
            return Err(AuthError::InvalidArgument(format!(
                "username must be between {MIN_USERNAME_LENGTH} and {MAX_USERNAME_LENGTH} characters"
            ))
            .into());
        }

        if req.password.len() < 8 || req.password.len() > MAX_PASSWORD_LENGTH {
            return Err(
                AuthError::InvalidArgument(format!("password must be between 8 and {MAX_PASSWORD_LENGTH} characters")).into(),
            );
        }

        let password_hash = Self::hash_password(req.password).await?;
        let user = self
            .state
            .store
            .create_user(req.email, req.username, password_hash)
            .await
            .map_err(Status::from)?;

        let tokens = self
            .issue_tokens(user.id, &user.email, &user.username)
            .await
            .map_err(Status::from)?;

        tracing::info!(user_id = %user.id, "User registered");

        Ok(Response::new(RegisterResponse {
            user_id: user.id.to_string(),
            email: user.email,
            username: user.username,
            tokens: Some(tokens),
        }))
    }

    #[tracing::instrument(skip_all, fields(email = %req.get_ref().email))]
    async fn login(&self, req: Request<LoginRequest>) -> Result<Response<LoginResponse>, Status> {
        let req = req.into_inner();

        if req.email.is_empty() || req.password.is_empty() {
            return Err(AuthError::InvalidArgument("email and password are required".into()).into());
        }

        let user = match self.state.store.find_user_by_email(&req.email).await.map_err(Status::from)? {
            Some(user) => user,
            None => {
                Self::dummy_verify().await;
                return Err(AuthError::InvalidCredentials.into());
            }
        };

        let password_hash = match user.password_hash {
            Some(ref hash) => hash.clone(),
            None => {
                Self::dummy_verify().await;
                return Err(AuthError::InvalidCredentials.into());
            }
        };

        Self::verify_password(req.password, password_hash)
            .await
            .map_err(Status::from)?;

        let tokens = self
            .issue_tokens(user.id, &user.email, &user.username)
            .await
            .map_err(Status::from)?;

        tracing::info!(user_id = %user.id, "User logged in");

        Ok(Response::new(LoginResponse {
            user_id: user.id.to_string(),
            email: user.email,
            username: user.username,
            tokens: Some(tokens),
        }))
    }

    #[tracing::instrument(skip_all)]
    async fn refresh_token(&self, req: Request<RefreshTokenRequest>) -> Result<Response<AuthTokens>, Status> {
        let req = req.into_inner();

        if req.refresh_token.is_empty() {
            return Err(AuthError::InvalidArgument("refresh_token is required".into()).into());
        }

        let claims = self
            .state
            .tokens
            .validate_refresh_token(&req.refresh_token)
            .map_err(Status::from)?;

        let jti: Uuid = claims
            .jti
            .parse()
            .map_err(|_| AuthError::InvalidToken("invalid jti".into()))
            .map_err(Status::from)?;

        let stored = self.state.store.consume_refresh_token(jti).await.map_err(Status::from)?;

        let token_user_id: Uuid = claims
            .sub
            .parse()
            .map_err(|_| AuthError::InvalidToken("invalid sub".into()))
            .map_err(Status::from)?;
        if token_user_id != stored.user_id {
            return Err(AuthError::InvalidToken("token subject mismatch".into()).into());
        }

        let user = self.state.store.find_user_by_id(stored.user_id).await.map_err(Status::from)?;

        let tokens = self
            .issue_tokens(user.id, &user.email, &user.username)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(tokens))
    }

    #[tracing::instrument(skip_all)]
    async fn validate_token(&self, req: Request<ValidateTokenRequest>) -> Result<Response<ValidateTokenResponse>, Status> {
        let req = req.into_inner();

        if req.access_token.is_empty() {
            return Err(AuthError::InvalidArgument("access_token is required".into()).into());
        }

        let claims = self
            .state
            .tokens
            .validate_access_token(&req.access_token)
            .map_err(Status::from)?;

        Ok(Response::new(ValidateTokenResponse {
            user_id: claims.sub,
            email: claims.email,
            username: claims.username,
            expires_at: to_timestamp(claims.exp as i64),
        }))
    }

    #[tracing::instrument(skip_all)]
    async fn logout(&self, req: Request<LogoutRequest>) -> Result<Response<()>, Status> {
        let req = req.into_inner();

        if req.refresh_token.is_empty() {
            return Err(AuthError::InvalidArgument("refresh_token is required".into()).into());
        }

        let claims = self
            .state
            .tokens
            .validate_refresh_token(&req.refresh_token)
            .map_err(Status::from)?;

        let jti: Uuid = claims
            .jti
            .parse()
            .map_err(|_| AuthError::InvalidToken("invalid jti".into()))
            .map_err(Status::from)?;

        self.state.store.revoke_refresh_token(jti).await.map_err(Status::from)?;

        Ok(Response::new(()))
    }

    #[tracing::instrument(skip_all, fields(provider = req.get_ref().provider))]
    async fn o_auth_get_auth_url(
        &self,
        req: Request<OAuthGetAuthUrlRequest>,
    ) -> Result<Response<OAuthGetAuthUrlResponse>, Status> {
        let req = req.into_inner();
        let provider = resolve_provider(req.provider)?;

        if req.redirect_uri.is_empty() {
            return Err(AuthError::InvalidArgument("redirect_uri is required".into()).into());
        }

        let authorize_url = self
            .state
            .oauth
            .get_authorize_url(provider, &req.redirect_uri)
            .map_err(Status::from)?;

        Ok(Response::new(OAuthGetAuthUrlResponse { authorize_url }))
    }

    #[tracing::instrument(skip_all, fields(provider = req.get_ref().provider))]
    async fn o_auth_authenticate(&self, req: Request<OAuthAuthenticateRequest>) -> Result<Response<AuthTokens>, Status> {
        let req = req.into_inner();
        let provider = resolve_provider(req.provider)?;

        if req.code.is_empty() || req.redirect_uri.is_empty() {
            return Err(AuthError::InvalidArgument("code and redirect_uri are required".into()).into());
        }

        let provider_str = provider.as_str_name();

        let user_info = self
            .state
            .oauth
            .exchange_code(provider, &req.code, &req.redirect_uri)
            .await
            .map_err(Status::from)?;

        let oauth_account = self
            .state
            .store
            .find_oauth_account(provider_str, &user_info.provider_user_id)
            .await
            .map_err(Status::from)?;

        let user = if let Some(account) = oauth_account {
            self.state
                .store
                .find_user_by_id(account.user_id)
                .await
                .map_err(Status::from)?
        } else {
            match self
                .state
                .store
                .find_user_by_email(&user_info.email)
                .await
                .map_err(Status::from)?
            {
                Some(existing_user) => {
                    if existing_user.password_hash.is_some() {
                        return Err(AuthError::OAuthAccountLinkDenied.into());
                    }
                    self.state
                        .store
                        .create_oauth_account(
                            existing_user.id,
                            provider_str.to_string(),
                            user_info.provider_user_id,
                            Some(user_info.email),
                        )
                        .await
                        .map_err(Status::from)?;
                    existing_user
                }
                None => {
                    let new_user = self
                        .state
                        .store
                        .create_oauth_user(user_info.email.clone(), user_info.name)
                        .await
                        .map_err(Status::from)?;

                    self.state
                        .store
                        .create_oauth_account(
                            new_user.id,
                            provider_str.to_string(),
                            user_info.provider_user_id,
                            Some(user_info.email),
                        )
                        .await
                        .map_err(Status::from)?;

                    new_user
                }
            }
        };

        let tokens = self
            .issue_tokens(user.id, &user.email, &user.username)
            .await
            .map_err(Status::from)?;

        tracing::info!(user_id = %user.id, provider = provider_str, "OAuth login successful");

        Ok(Response::new(tokens))
    }
}

fn resolve_provider(value: i32) -> Result<OAuthProvider, Status> {
    match OAuthProvider::try_from(value) {
        Ok(OAuthProvider::OauthProviderUnspecified) | Err(_) => {
            Err(AuthError::InvalidArgument("unsupported OAuth provider".into()).into())
        }
        Ok(p) => Ok(p),
    }
}

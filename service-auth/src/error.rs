use tonic::{Code, Status};

pub type AuthResult<T> = Result<T, AuthError>;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("User already exists")]
    UserAlreadyExists,
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("User not found")]
    UserNotFound,
    #[error("Token not found or already revoked")]
    TokenNotFound,
    #[error("Token has expired")]
    TokenExpired,
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Password hashing failed: {0}")]
    HashingError(String),
    #[error("OAuth error: {0}")]
    OAuthError(String),
    #[error("An account with this email already exists, log in with password to link OAuth")]
    OAuthAccountLinkDenied,
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<AuthError> for Status {
    fn from(err: AuthError) -> Self {
        let code = match &err {
            AuthError::UserAlreadyExists => Code::AlreadyExists,
            AuthError::InvalidCredentials => Code::Unauthenticated,
            AuthError::UserNotFound => Code::NotFound,
            AuthError::TokenNotFound => Code::Unauthenticated,
            AuthError::TokenExpired => Code::Unauthenticated,
            AuthError::InvalidToken(_) => Code::Unauthenticated,
            AuthError::InvalidArgument(_) => Code::InvalidArgument,
            AuthError::OAuthAccountLinkDenied => Code::PermissionDenied,
            AuthError::OAuthError(msg) => {
                tracing::error!("OAuth error: {msg}");
                Code::Internal
            }
            AuthError::HashingError(msg) => {
                tracing::error!("Password hashing failed: {msg}");
                Code::Internal
            }
            AuthError::Internal(msg) => {
                tracing::error!("Internal error: {msg}");
                Code::Internal
            }
        };

        let message = match &err {
            AuthError::OAuthError(_) | AuthError::HashingError(_) | AuthError::Internal(_) => "Internal server error".into(),
            _ => err.to_string(),
        };

        Status::new(code, message)
    }
}

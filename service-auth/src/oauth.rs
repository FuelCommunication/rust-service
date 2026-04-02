use std::time::Duration;

use serde::Deserialize;

use crate::config::Config;
use crate::error::AuthError;
use crate::proto::OAuthProvider;

pub struct OAuthUserInfo {
    pub provider_user_id: String,
    pub email: String,
    pub name: String,
}

struct ProviderConfig {
    client_id: String,
    client_secret: String,
}

pub struct OAuthManager {
    http: reqwest::Client,
    google: Option<ProviderConfig>,
    github: Option<ProviderConfig>,
    allowed_redirect_origins: Vec<String>,
}

impl OAuthManager {
    pub fn new(config: &Config) -> Self {
        let google = match (&config.google_client_id, &config.google_client_secret) {
            (Some(id), Some(secret)) => Some(ProviderConfig {
                client_id: id.clone(),
                client_secret: secret.clone(),
            }),
            _ => None,
        };

        let github = match (&config.github_client_id, &config.github_client_secret) {
            (Some(id), Some(secret)) => Some(ProviderConfig {
                client_id: id.clone(),
                client_secret: secret.clone(),
            }),
            _ => None,
        };

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http,
            google,
            github,
            allowed_redirect_origins: config.allowed_redirect_origins.clone(),
        }
    }

    fn validate_redirect_uri(&self, redirect_uri: &str) -> Result<(), AuthError> {
        if self.allowed_redirect_origins.is_empty() {
            return Ok(());
        }
        let origin = extract_origin(redirect_uri);
        if self.allowed_redirect_origins.iter().any(|allowed| allowed == &origin) {
            Ok(())
        } else {
            Err(AuthError::InvalidArgument(format!(
                "redirect_uri origin '{origin}' is not allowed"
            )))
        }
    }

    pub fn get_authorize_url(&self, provider: OAuthProvider, redirect_uri: &str) -> Result<String, AuthError> {
        self.validate_redirect_uri(redirect_uri)?;
        match provider {
            OAuthProvider::OauthProviderGoogle => {
                let cfg = self
                    .google
                    .as_ref()
                    .ok_or_else(|| AuthError::OAuthError("Google OAuth is not configured".into()))?;
                Ok(format!(
                    "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile&access_type=offline",
                    urlencoding::encode(&cfg.client_id),
                    urlencoding::encode(redirect_uri),
                ))
            }
            OAuthProvider::OauthProviderGithub => {
                let cfg = self
                    .github
                    .as_ref()
                    .ok_or_else(|| AuthError::OAuthError("GitHub OAuth is not configured".into()))?;
                Ok(format!(
                    "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user%20user:email",
                    urlencoding::encode(&cfg.client_id),
                    urlencoding::encode(redirect_uri),
                ))
            }
            OAuthProvider::OauthProviderUnspecified => Err(AuthError::InvalidArgument("OAuth provider is required".into())),
        }
    }

    pub async fn exchange_code(
        &self,
        provider: OAuthProvider,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthUserInfo, AuthError> {
        self.validate_redirect_uri(redirect_uri)?;
        match provider {
            OAuthProvider::OauthProviderGoogle => self.google_exchange(code, redirect_uri).await,
            OAuthProvider::OauthProviderGithub => self.github_exchange(code, redirect_uri).await,
            OAuthProvider::OauthProviderUnspecified => Err(AuthError::InvalidArgument("OAuth provider is required".into())),
        }
    }

    async fn google_exchange(&self, code: &str, redirect_uri: &str) -> Result<OAuthUserInfo, AuthError> {
        let cfg = self
            .google
            .as_ref()
            .ok_or_else(|| AuthError::OAuthError("Google OAuth is not configured".into()))?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
        }

        let token_resp: TokenResponse = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("code", code),
                ("client_id", &cfg.client_id),
                ("client_secret", &cfg.client_secret),
                ("redirect_uri", redirect_uri),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await
            .map_err(|e| AuthError::OAuthError(format!("Google token exchange failed: {e}")))?
            .error_for_status()
            .map_err(|e| AuthError::OAuthError(format!("Google token exchange returned error: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("Google token response parse failed: {e}")))?;

        #[derive(Deserialize)]
        struct UserInfo {
            id: String,
            email: String,
            name: Option<String>,
        }

        let user_info: UserInfo = self
            .http
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .bearer_auth(&token_resp.access_token)
            .send()
            .await
            .map_err(|e| AuthError::OAuthError(format!("Google userinfo request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AuthError::OAuthError(format!("Google userinfo returned error: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("Google userinfo parse failed: {e}")))?;

        Ok(OAuthUserInfo {
            provider_user_id: user_info.id,
            email: user_info.email,
            name: user_info.name.unwrap_or_default(),
        })
    }

    async fn github_exchange(&self, code: &str, redirect_uri: &str) -> Result<OAuthUserInfo, AuthError> {
        let cfg = self
            .github
            .as_ref()
            .ok_or_else(|| AuthError::OAuthError("GitHub OAuth is not configured".into()))?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
        }

        let token_resp: TokenResponse = self
            .http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("code", code),
                ("client_id", cfg.client_id.as_str()),
                ("client_secret", cfg.client_secret.as_str()),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .map_err(|e| AuthError::OAuthError(format!("GitHub token exchange failed: {e}")))?
            .error_for_status()
            .map_err(|e| AuthError::OAuthError(format!("GitHub token exchange returned error: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("GitHub token response parse failed: {e}")))?;

        #[derive(Deserialize)]
        struct GitHubUser {
            id: i64,
            login: String,
            name: Option<String>,
        }

        let user: GitHubUser = self
            .http
            .get("https://api.github.com/user")
            .header("User-Agent", "service-auth")
            .bearer_auth(&token_resp.access_token)
            .send()
            .await
            .map_err(|e| AuthError::OAuthError(format!("GitHub user request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AuthError::OAuthError(format!("GitHub user request returned error: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("GitHub user parse failed: {e}")))?;

        #[derive(Deserialize)]
        struct GitHubEmail {
            email: String,
            primary: bool,
            verified: bool,
        }

        let emails: Vec<GitHubEmail> = self
            .http
            .get("https://api.github.com/user/emails")
            .header("User-Agent", "service-auth")
            .bearer_auth(&token_resp.access_token)
            .send()
            .await
            .map_err(|e| AuthError::OAuthError(format!("GitHub emails request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AuthError::OAuthError(format!("GitHub emails request returned error: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("GitHub emails parse failed: {e}")))?;

        let email = emails
            .into_iter()
            .find(|e| e.primary && e.verified)
            .map(|e| e.email)
            .ok_or_else(|| AuthError::OAuthError("No verified primary email found on GitHub account".into()))?;

        Ok(OAuthUserInfo {
            provider_user_id: user.id.to_string(),
            email,
            name: user.name.unwrap_or(user.login),
        })
    }
}

fn extract_origin(uri: &str) -> String {
    let after_scheme = uri.find("://").map(|i| i + 3).unwrap_or(0);
    let path_start = uri[after_scheme..].find('/').map(|i| i + after_scheme).unwrap_or(uri.len());
    uri[..path_start].to_string()
}

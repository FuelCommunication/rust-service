use std::sync::Arc;

use crate::oauth::OAuthManager;
use crate::store::AuthStore;
use crate::token::TokenManager;

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub store: AuthStore,
    pub tokens: Arc<TokenManager>,
    pub oauth: OAuthManager,
}

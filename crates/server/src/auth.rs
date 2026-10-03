//! Authentication: one-time email codes, passwords, sessions and request extractors.

pub mod oidc;
pub mod rate_limit;
pub mod secrets;
mod session;

pub use session::{
    CLIENT_HEADER, ClientIp, CurrentPlayer, CurrentUser, SESSION_COOKIE, create_session,
    ensure_player, wants_cookie,
};

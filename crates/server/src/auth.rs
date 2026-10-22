//! Authentication: one-time email codes, passwords, sessions and request extractors.

pub mod oidc;
mod platform;
pub mod rate_limit;
pub mod secrets;
mod session;

pub use platform::{PlatformOperator, PlatformToken};
pub use session::{
    CLIENT_HEADER, ClientIp, CurrentPlayer, CurrentUser, SESSION_COOKIE, SessionDelivery,
    create_session, ensure_player,
};

//! Only explicitly classified, static messages may cross the API boundary.
use axum::http::StatusCode;
use std::fmt;

#[derive(Debug)]
pub struct UserFacing {
    pub status: StatusCode,
    pub message: &'static str,
}
impl UserFacing {
    pub fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
    pub fn forbidden(message: &'static str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message,
        }
    }
    pub fn not_found(message: &'static str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message,
        }
    }
}
impl fmt::Display for UserFacing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}
impl std::error::Error for UserFacing {}

#[macro_export]
macro_rules! ensure_public {
    ($condition:expr, $message:literal $(,)?) => {
        if !$condition {
            return Err($crate::error::UserFacing::bad_request($message).into());
        }
    };
}

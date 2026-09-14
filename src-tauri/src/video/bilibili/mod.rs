//! B 站平台适配：登录、元信息、字幕与媒体地址。

pub(crate) mod avatar;
mod cookies;
pub(crate) mod download;
pub(crate) mod http;
mod http_errors;
pub(crate) mod login;
pub(crate) mod media;
mod metadata;
mod subtitle;
mod targets;
pub(crate) mod wbi;

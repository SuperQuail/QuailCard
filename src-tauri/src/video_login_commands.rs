//! B 站登录命令：恢复、扫码确认与退出共享加密保险库提交边界。
use crate::{
    error::CommandError,
    services::AppServices,
    storage::Storage,
    video::{
        bilibili::http::{BiliClient, CookieJar},
        models::VideoLoginStatus,
    },
    video_bridge::{self, BILI_CREDENTIAL},
};
use tauri::{Manager, Window};
use zeroize::Zeroizing;

/// 开始扫码登录；已有会话时直接返回其状态。
#[tauri::command]
pub async fn video_login_start(
    app: tauri::AppHandle,
    window: Window,
) -> Result<VideoLoginStatus, CommandError> {
    let services = app.state::<AppServices>();
    let state = services.video_login.status(window.label()).state;
    if state == "confirmed" {
        match video_login_status(app.clone(), window.clone()).await {
            Ok(status) => return Ok(status),
            Err(error) if login_missing(&error) => {}
            Err(error) => return Err(error),
        }
    } else if state == "idle" {
        // 已保存的凭据必须过主站校验，同时把昵称与头像带回界面。
        match restored(&app).await {
            Ok(Some(profile)) => {
                return Ok(VideoLoginStatus {
                    state: "confirmed".into(),
                    message: "已恢复保存的 B 站登录".into(),
                    image: String::new(),
                    logged_in: true,
                    name: profile.name,
                    avatar: profile.avatar,
                })
            }
            Ok(None) => {}
            Err(error) if login_missing(&error) => {}
            Err(error) => return Err(error),
        }
    }
    let client = BiliClient::new(CookieJar::default())?;
    services.video_login.start(window.label(), client)
}

/// 查询扫码状态；确认凭据通过主站校验且落库后才报告登录成功。
#[tauri::command]
pub async fn video_login_status(
    app: tauri::AppHandle,
    window: Window,
) -> Result<VideoLoginStatus, CommandError> {
    let services = app.state::<AppServices>();
    let epoch = services.video_login.epoch();
    let mut status = services.video_login.status(window.label());
    status.logged_in = false;
    if status.state == "confirmed" {
        let jar = services
            .video_login
            .take_cookie(window.label())
            .ok_or_else(video_bridge::credentials::invalid_credential)?;
        let client = BiliClient::new(jar)?;
        let profile = video_bridge::credentials::validate(&client)
            .await?
            .ok_or_else(video_bridge::credentials::invalid_credential)?;
        // nav 校验也可能补发设备字段，一并保存以供后续接口使用。
        persist_cookie(&app, &client.session_cookie(), window.label(), epoch).await?;
        status.logged_in = true;
        status.name = profile.name;
        status.avatar = profile.avatar;
    } else if status.state == "idle" {
        // 扫码进行中不读取保险库、不额外轮询主站；空凭据也不会访问网络。
        let client = video_bridge::client(&app).await?;
        match video_bridge::credentials::validate(&client).await {
            Ok(Some(profile)) => {
                status.logged_in = true;
                status.name = profile.name;
                status.avatar = profile.avatar;
            }
            // 明确未登录或凭据失效时不能凭 Cookie 存在就宣称已登录。
            Ok(None) => status.logged_in = false,
            Err(error) if login_missing(&error) => status.logged_in = false,
            // 网络或风控故障不清空本地登录判断，具体接口会再给出准确错误。
            Err(_) => status.logged_in = client.cookie().is_logged_in(),
        }
    }
    Ok(status)
}

/// 读取已登录用户的头像并代理为 data URL；未登录或不可用时返回空串。
#[tauri::command]
pub async fn video_avatar(app: tauri::AppHandle) -> Result<String, CommandError> {
    let client = video_bridge::client(&app).await?;
    let Some(profile) = video_bridge::credentials::validate(&client)
        .await
        .ok()
        .flatten()
    else {
        return Ok(String::new());
    };
    if profile.avatar.is_empty() {
        return Ok(String::new());
    }
    // 头像只是展示信息，失败不应让整个登录态查询失败。
    Ok(
        crate::video::bilibili::avatar::fetch(&client, &profile.avatar)
            .await
            .unwrap_or_default(),
    )
}

/// 已保存凭据的主站校验；None 表示确认未登录。
async fn restored(
    app: &tauri::AppHandle,
) -> Result<Option<video_bridge::credentials::LoginProfile>, CommandError> {
    let client = video_bridge::client(app).await?;
    video_bridge::credentials::validate(&client).await
}

/// 只有明确的未登录或凭据失效才允许重新扫码，其它故障必须如实上报。
fn login_missing(error: &CommandError) -> bool {
    matches!(
        error.code,
        "VIDEO_LOGIN_REQUIRED" | "VIDEO_CREDENTIAL_INVALID" | "VIDEO_LOGIN_CREDENTIAL_MISSING"
    )
}

/// 取消扫码会话。
#[tauri::command]
pub fn video_login_cancel(app: tauri::AppHandle, window: Window) -> Result<(), CommandError> {
    app.state::<AppServices>()
        .video_login
        .finish(window.label());
    Ok(())
}

/// 退出登录：只删除 B 站凭据，不影响模型供应商。
#[tauri::command]
pub async fn video_logout(app: tauri::AppHandle) -> Result<(), CommandError> {
    let storage = app.state::<Storage>();
    let services = app.state::<AppServices>();
    let _guard = services.vault.lock_operations().await;
    services.video_login.logout();
    let envelope = services
        .vault
        .prepare_delete_credential(&storage, BILI_CREDENTIAL)
        .await?;
    storage.save_vault_envelope(&envelope).await
}

/// 把扫码结果写入加密保险库。
async fn persist_cookie(
    app: &tauri::AppHandle,
    jar: &CookieJar,
    owner: &str,
    epoch: u64,
) -> Result<(), CommandError> {
    let storage = app.state::<Storage>();
    let services = app.state::<AppServices>();
    let credential = Zeroizing::new(jar.encode());
    let _guard = services.vault.lock_operations().await;
    if services.video_login.epoch() != epoch {
        return Err(CommandError::new(
            "VIDEO_LOGIN_CANCELLED",
            "登录状态已改变，请重新查询",
        ));
    }
    let envelope = services
        .vault
        .prepare_set_credential(&storage, BILI_CREDENTIAL, &credential, None)
        .await?;
    storage.save_vault_envelope(&envelope).await?;
    services.video_login.acknowledge_cookie(owner);
    Ok(())
}

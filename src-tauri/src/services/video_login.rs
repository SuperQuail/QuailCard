//! 扫码登录会话：后台轮询二维码状态，成功后把 Cookie 交给命令层持久化。

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crate::{
    error::CommandError,
    video::{
        bilibili::{
            http::{BiliClient, CookieJar},
            login::{self, LoginState},
        },
        models::VideoLoginStatus,
    },
};

/// 会话安全上限；B 站自己的二维码有效期约三分钟，这里留出余量，
/// 真正的过期以服务端 86038 为准，避免我们比服务端先判定过期。
const SESSION_TTL: Duration = Duration::from_secs(300);

/// 单个登录会话。
struct Session {
    status: VideoLoginStatus,
    cookie: Option<CookieJar>,
    started: Instant,
    finished: bool,
}

/// 按窗口隔离的登录会话表。
#[derive(Default)]
pub(crate) struct VideoLoginSessions {
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
    /// 已确认但尚未落库的 Cookie；弹窗提前关闭也不会丢，下次查询时补写。
    pending: Arc<Mutex<HashMap<String, CookieJar>>>,
    epoch: AtomicU64,
}

impl VideoLoginSessions {
    /// 凭据提交检查版本；退出后迟到的 nav 响应不得重新写回账号。
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// 凭据是全局的；持有保险库操作锁时撤销所有窗口扫码与待提交结果。
    pub(crate) fn logout(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        for session in sessions.values() {
            session.lock().unwrap_or_else(|e| e.into_inner()).finished = true;
        }
        sessions.clear();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// 启动或复用会话；只有进行中的会话才复用，终态不可复用。
    pub(crate) fn start(
        &self,
        owner: &str,
        client: BiliClient,
    ) -> Result<VideoLoginStatus, CommandError> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        sessions.retain(|_, session| {
            session
                .lock()
                .map(|session| !session.finished || session.started.elapsed() < SESSION_TTL)
                .unwrap_or(false)
        });
        if let Some(existing) = sessions.get(owner) {
            let session = existing.lock().unwrap_or_else(|poison| poison.into_inner());
            if reusable(&session.status.state, session.finished) {
                return Ok(session.status.clone());
            }
        }
        // 明确重新获取时丢弃上一轮失败凭据，避免旧 pending 遮住新会话。
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(owner);
        let session = Arc::new(Mutex::new(Session {
            status: VideoLoginStatus {
                state: "starting".to_string(),
                message: "正在获取二维码".to_string(),
                ..Default::default()
            },
            cookie: None,
            started: Instant::now(),
            finished: false,
        }));
        sessions.insert(owner.to_string(), session.clone());
        drop(sessions);
        let pending = self.pending.clone();
        let owner = owner.to_string();
        tauri::async_runtime::spawn(poll_loop(session, pending, owner, client));
        Ok(VideoLoginStatus {
            state: "starting".to_string(),
            message: "正在获取二维码".to_string(),
            ..Default::default()
        })
    }

    /// 读取当前会话状态；有待落库的 Cookie 时直接报告已登录。
    pub(crate) fn status(&self, owner: &str) -> VideoLoginStatus {
        let confirmed = self
            .pending
            .lock()
            .map(|store| store.contains_key(owner))
            .unwrap_or(false);
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut status = sessions
            .get(owner)
            .and_then(|session| session.lock().ok().map(|session| session.status.clone()))
            .unwrap_or_else(|| VideoLoginStatus {
                state: "idle".to_string(),
                message: "未开始扫码".to_string(),
                ..Default::default()
            });
        if confirmed {
            status.state = "confirmed".to_string();
            status.message = "登录成功".to_string();
            status.logged_in = true;
        }
        status
    }

    /// 读取待保存凭据快照；校验或写库失败时保留，成功后显式确认移除。
    pub(crate) fn take_cookie(&self, owner: &str) -> Option<CookieJar> {
        if let Ok(store) = self.pending.lock() {
            if let Some(jar) = store.get(owner) {
                return Some(jar.clone());
            }
        }
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let session = sessions.get(owner)?.lock().ok()?;
        session.cookie.clone()
    }

    /// 仅在凭据成功写库后清理待保存副本，取消弹窗不调用此方法。
    pub(crate) fn acknowledge_cookie(&self, owner: &str) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(owner);
        self.finish(owner);
    }

    /// 结束会话（用户取消或完成落库后调用）。
    pub(crate) fn finish(&self, owner: &str) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(session) = sessions.get(owner) {
            if let Ok(mut session) = session.lock() {
                session.finished = true;
            }
        }
        sessions.retain(|key, _| key != owner);
    }
}

/// 每秒轮询一次二维码状态，直到确认、过期或超时。
async fn poll_loop(
    session: Arc<Mutex<Session>>,
    pending: Arc<Mutex<HashMap<String, CookieJar>>>,
    owner: String,
    client: BiliClient,
) {
    let start = match login::start(&client).await {
        Ok(start) => start,
        Err(error) => {
            set_terminal(&session, "failed", &error.message);
            return;
        }
    };
    set_state_with_image(&session, "waiting", "请使用 B 站客户端扫码", &start.image);
    loop {
        {
            let Ok(current) = session.lock() else {
                return;
            };
            if current.finished {
                return;
            }
            if current.started.elapsed() > SESSION_TTL {
                drop(current);
                set_terminal(&session, "timeout", "登录超时，请重新获取二维码");
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let result = login::poll(&client, &start.key).await;
        // 取消可能发生在 HTTP 请求期间，旧请求不能写回待保存凭据。
        if session
            .lock()
            .map(|current| current.finished)
            .unwrap_or(true)
        {
            return;
        }
        match result {
            Ok(LoginState::Waiting) => {}
            Ok(LoginState::Scanned) => set_state(&session, "scanned", "已扫码，请在手机上确认", ""),
            Ok(LoginState::Expired) => {
                set_terminal(&session, "expired", "二维码已过期，请重新获取");
                return;
            }
            Ok(LoginState::Confirmed(jar)) => {
                // 与取消共用会话锁，避免检查 finished 后被退出插入而复活 pending。
                if let Ok(mut current) = session.lock() {
                    if current.finished {
                        return;
                    }
                    pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(owner.clone(), (*jar).clone());
                    current.cookie = Some(*jar);
                    current.status.state = "confirmed".to_string();
                    current.status.message = "登录成功".to_string();
                    current.status.logged_in = true;
                }
                return;
            }
            Err(error) => {
                set_terminal(&session, "failed", &error.message);
                return;
            }
        }
    }
}

/// 会话是否还能复用：只有进行中的状态才复用，终态必须重新申请二维码。
///
/// 之前终态没有标记结束，导致「重新获取」拿到旧会话、二维码永远不刷新；
/// 用户在旧二维码上完成登录后应用也拿不到 Cookie，只能一直显示已过期。
pub(crate) fn reusable(state: &str, finished: bool) -> bool {
    !finished && matches!(state, "starting" | "waiting" | "scanned")
}

/// 写终态：清空二维码并标记结束，避免继续扫描一张已经失效的图。
fn set_terminal(session: &Arc<Mutex<Session>>, state: &str, message: &str) {
    if let Ok(mut current) = session.lock() {
        current.status.state = state.to_string();
        current.status.message = message.to_string();
        current.status.image = String::new();
        current.finished = true;
    }
}

/// 更新状态；消息为空时保留原二维码。
fn set_state(session: &Arc<Mutex<Session>>, state: &str, message: &str, _image: &str) {
    if let Ok(mut current) = session.lock() {
        current.status.state = state.to_string();
        current.status.message = message.to_string();
    }
}

/// 更新状态并替换二维码图片。
fn set_state_with_image(session: &Arc<Mutex<Session>>, state: &str, message: &str, image: &str) {
    if let Ok(mut current) = session.lock() {
        current.status.state = state.to_string();
        current.status.message = message.to_string();
        current.status.image = image.to_string();
    }
}

#[cfg(test)]
#[path = "video_login_tests.rs"]
mod tests;

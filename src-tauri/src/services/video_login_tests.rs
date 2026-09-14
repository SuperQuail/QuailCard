//! 扫码会话与退出版本的无网络回归测试。
use super::*;

#[test]
/// 全局退出同时撤销各窗口待保存 Cookie 和迟到校验的提交版本。
fn logout_invalidates_pending_credentials_and_epoch() {
    let sessions = VideoLoginSessions::default();
    let epoch = sessions.epoch();
    for owner in ["main", "second"] {
        sessions
            .pending
            .lock()
            .unwrap()
            .insert(owner.into(), CookieJar::decode("SESSDATA=fake"));
    }
    let session = Arc::new(Mutex::new(Session {
        status: VideoLoginStatus::default(),
        cookie: None,
        started: Instant::now(),
        finished: false,
    }));
    sessions
        .sessions
        .lock()
        .unwrap()
        .insert("main".into(), session.clone());
    sessions.logout();
    assert_ne!(sessions.epoch(), epoch);
    assert!(session.lock().unwrap().finished);
    for owner in ["main", "second"] {
        assert!(sessions.take_cookie(owner).is_none());
        assert_eq!(sessions.status(owner).state, "idle");
    }
}

#[test]
/// 进行中的会话可以复用，避免重复申请二维码。
fn active_sessions_are_reusable() {
    assert!(reusable("starting", false));
    assert!(reusable("waiting", false));
    assert!(reusable("scanned", false));
}

#[test]
/// 终态必须重新申请二维码，否则用户会卡在过期提示上。
fn terminal_sessions_are_not_reusable() {
    assert!(!reusable("expired", false));
    assert!(!reusable("timeout", false));
    assert!(!reusable("failed", false));
    assert!(!reusable("waiting", true));
    assert!(!reusable("confirmed", true));
}

#[test]
/// 终态会清空二维码并标记结束。
fn terminal_state_clears_image() {
    let session = Arc::new(Mutex::new(Session {
        status: VideoLoginStatus {
            state: "waiting".to_string(),
            message: "扫码".to_string(),
            image: "data:image/svg+xml;base64,abc".to_string(),
            logged_in: false,
            name: "测试用户".to_string(),
            avatar: String::new(),
        },
        cookie: None,
        started: Instant::now(),
        finished: false,
    }));
    set_terminal(&session, "expired", "二维码已过期，请重新获取");
    let current = session.lock().unwrap();
    assert!(current.finished);
    assert!(current.status.image.is_empty());
    assert_eq!(current.status.state, "expired");
}

#[test]
/// 关闭弹窗后的待保存 Cookie 在写库确认前始终可以重试读取。
fn pending_cookie_survives_closed_dialog() {
    let sessions = VideoLoginSessions::default();
    let jar = CookieJar::decode("SESSDATA=s");
    sessions
        .pending
        .lock()
        .unwrap()
        .insert("main".to_string(), jar.clone());
    let status = sessions.status("main");
    assert_eq!(status.state, "confirmed");
    assert!(status.logged_in);
    assert_eq!(sessions.take_cookie("main").unwrap().sessdata, "s");
    // 校验或保存失败可以再次获取，只有保存成功才清理。
    assert!(sessions.take_cookie("main").is_some());
    sessions.acknowledge_cookie("main");
    assert!(sessions.take_cookie("main").is_none());
    // 队列清空后回到空闲状态，不会重复上报。
    assert_eq!(sessions.status("main").state, "idle");
}

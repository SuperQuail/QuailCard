//! 请求构建和拒绝路径回归：不放宽生产目的地白名单来迁就本地模拟服务。
use super::*;

#[test]
/// 真实请求构建必须保留设备 Cookie，敏感标记阻止调试输出值。
fn trusted_api_sends_sensitive_cookie_and_browser_headers() {
    let client = BiliClient::new(CookieJar::decode(
        "SESSDATA=fake%2Csession; buvid3=three; buvid4=four",
    ))
    .unwrap();
    let target =
        super::super::targets::validate("https://api.bilibili.com/x/web-interface/nav").unwrap();
    let request = client
        .request(&target, REFERER, true)
        .unwrap()
        .build()
        .unwrap();
    let headers = request.headers();
    let cookie = &headers[reqwest::header::COOKIE];
    assert!(cookie.is_sensitive());
    assert!(cookie.to_str().unwrap().contains("SESSDATA=fake%2Csession"));
    assert!(cookie.to_str().unwrap().contains("buvid4=four"));
    assert_eq!(headers[reqwest::header::ORIGIN], ORIGIN);
    assert_eq!(headers[reqwest::header::REFERER], REFERER);
    assert_eq!(headers[reqwest::header::USER_AGENT], USER_AGENT);
    assert!(!format!("{request:?}").contains("fake%2Csession"));
}

#[test]
/// 同一会话访问 CDN、短链和移动页均不能携带凭据，外部 Referer 被替换。
fn noncredential_targets_never_receive_cookie() {
    let client = BiliClient::new(CookieJar::decode("SESSDATA=secret")).unwrap();
    for raw in [
        "https://upos.bilivideo.com/video",
        "https://aisubtitle.hdslb.com/subtitle",
        "https://b23.tv/abc",
        "https://m.bilibili.com/video/av1",
    ] {
        let target = super::super::targets::validate(raw).unwrap();
        let request = client
            .request(&target, "https://evil.test/secret", true)
            .unwrap()
            .build()
            .unwrap();
        assert!(!request.headers().contains_key(reqwest::header::COOKIE));
        assert_eq!(request.headers()[reqwest::header::REFERER], REFERER);
    }
}

#[tokio::test]
/// 在联网之前拒绝非受信地址，不返回 URL 密钥，所有公开读取入口共用校验。
async fn rejects_unsafe_requests_before_network() {
    let client = BiliClient::new(CookieJar::decode("SESSDATA=secret")).unwrap();
    let url = "http://127.0.0.1:1/?qrcode_key=secret";
    for error in [
        client.api(url).await.unwrap_err(),
        client.text(url, "").await.unwrap_err(),
        client.stream(url, "").await.unwrap_err(),
        client.login_response(url).await.unwrap_err(),
    ] {
        assert_eq!(error.code, "VIDEO_URL_INVALID");
        assert!(!error.message.contains("secret"));
    }
}

#[tokio::test]
/// 多段 Range 和控制字符不能进入代理上游请求。
async fn rejects_invalid_ranges() {
    let client = BiliClient::new(CookieJar::default()).unwrap();
    for range in [
        "bytes=0-1,2-3",
        "bytes=-",
        "bytes=a-b",
        "bytes=0-1\r\nCookie: secret",
    ] {
        assert_eq!(
            client
                .stream_range("https://upos.bilivideo.com/video", REFERER, Some(range))
                .await
                .unwrap_err()
                .code,
            "VIDEO_URL_INVALID"
        );
    }
}

#[test]
/// 二维码阶段设备字段保留到确认，后续响应只覆盖返回字段。
fn session_cookie_merges_without_dropping_device_fields() {
    let client = BiliClient::new(CookieJar::decode("buvid3=three; buvid4=four")).unwrap();
    client
        .received
        .lock()
        .unwrap()
        .merge(&CookieJar::decode("SESSDATA=fake; b_nut=nut; b_lsid=lsid"));
    let jar = client.session_cookie();
    assert_eq!(jar.buvid3, "three");
    assert_eq!(jar.buvid4, "four");
    assert_eq!(jar.b_nut, "nut");
    assert_eq!(jar.b_lsid, "lsid");
    assert!(jar.is_logged_in());
}

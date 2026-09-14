//! B 站 Cookie 白名单编解码，兼容旧凭据并保留设备信息。

use reqwest::header::{HeaderMap, SET_COOKIE};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Default, PartialEq)]
pub(crate) struct CookieJar {
    pub sessdata: String,
    pub bili_jct: String,
    pub dede_user_id: String,
    pub dede_user_id_ckmd5: String,
    pub buvid3: String,
    pub buvid4: String,
    pub b_nut: String,
    pub b_lsid: String,
}

impl CookieJar {
    /// 字段名统一注册，读写和清理共享白名单，未知 Cookie 不落库。
    fn fields(&self) -> [(&'static str, &String); 8] {
        [
            ("SESSDATA", &self.sessdata),
            ("bili_jct", &self.bili_jct),
            ("DedeUserID", &self.dede_user_id),
            ("DedeUserID__ckMd5", &self.dede_user_id_ckmd5),
            ("buvid3", &self.buvid3),
            ("buvid4", &self.buvid4),
            ("b_nut", &self.b_nut),
            ("b_lsid", &self.b_lsid),
        ]
    }

    /// 可变字段注册用于合并和清理，保持与序列化顺序一致。
    fn fields_mut(&mut self) -> [(&'static str, &mut String); 8] {
        [
            ("SESSDATA", &mut self.sessdata),
            ("bili_jct", &mut self.bili_jct),
            ("DedeUserID", &mut self.dede_user_id),
            ("DedeUserID__ckMd5", &mut self.dede_user_id_ckmd5),
            ("buvid3", &mut self.buvid3),
            ("buvid4", &mut self.buvid4),
            ("b_nut", &mut self.b_nut),
            ("b_lsid", &mut self.b_lsid),
        ]
    }

    /// 仅表示本地有凭据；是否有效必须由 nav 校验，不能用它代替服务端登录态。
    pub(crate) fn is_logged_in(&self) -> bool {
        !self.sessdata.is_empty()
    }

    /// 拼装白名单 Cookie；调用方负责保护返回的凭据字符串。
    pub(crate) fn header(&self) -> Option<String> {
        let mut raw = String::new();
        for (name, value) in self.fields() {
            if value.is_empty() || !safe_value(value) {
                continue;
            }
            if !raw.is_empty() {
                raw.push_str("; ");
            }
            raw.push_str(name);
            raw.push('=');
            raw.push_str(value);
        }
        if raw.is_empty() {
            None
        } else {
            Some(raw)
        }
    }

    /// 保持旧版字符串格式，添加字段不要求迁移保险库。
    pub(crate) fn encode(&self) -> String {
        self.header().unwrap_or_default()
    }

    /// 按白名单读取；保留百分号编码与等号，不分配未知字段的秘密副本。
    pub(crate) fn decode(raw: &str) -> Self {
        let mut jar = Self::default();
        for part in raw.split(';') {
            jar.apply_pair(part);
        }
        jar
    }

    /// 登录回调 URL 只读取查询参数，片段不会混入 Cookie。
    pub(crate) fn from_login_url(url: &str) -> Self {
        let mut jar = Self::default();
        if !super::targets::validate(url).is_ok_and(|url| super::targets::credentials(&url)) {
            return jar;
        }
        if let Some((_, query)) = url.split_once('?') {
            for pair in query.split('#').next().unwrap_or("").split('&') {
                jar.apply_pair(pair);
            }
        }
        jar
    }

    /// 合并已收到的字段，缺失字段不覆盖之前的设备 Cookie。
    pub(crate) fn merge(&mut self, other: &Self) {
        let raw = Zeroizing::new(other.encode());
        for pair in raw.split(';') {
            self.apply_pair(pair);
        }
    }

    /// 逐个读取 Set-Cookie，不能按逗号拆分含 Expires 的响应头。
    pub(crate) fn absorb(&mut self, headers: &HeaderMap) {
        for header in headers.get_all(SET_COOKIE) {
            if let Ok(value) = header.to_str() {
                self.apply_pair(value.split(';').next().unwrap_or(""));
            }
        }
    }

    /// 替换前清理旧值，并拒绝控制字符，防止 Cookie 头注入。
    fn apply_pair(&mut self, pair: &str) {
        let Some((name, value)) = pair.trim().split_once('=') else {
            return;
        };
        let value = value.trim();
        if !safe_value(value) {
            return;
        }
        if let Some((_, field)) = self
            .fields_mut()
            .into_iter()
            .find(|(key, _)| *key == name.trim())
        {
            field.zeroize();
            field.push_str(value);
        }
    }
}

/// RFC Cookie 值白名单防止分号注入新字段，编码令牌保持原样。
fn safe_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, 0x21 | 0x23..=0x2B | 0x2D..=0x3A | 0x3C..=0x5B | 0x5D..=0x7E))
}

impl Drop for CookieJar {
    /// 会话销毁时清理登录和设备信息。
    fn drop(&mut self) {
        for (_, value) in self.fields_mut() {
            value.zeroize();
        }
    }
}

impl std::fmt::Debug for CookieJar {
    /// 调试与断言失败不得输出凭据明文。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieJar")
            .field("has_credentials", &self.is_logged_in())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    /// 老凭据仍可读取，新设备字段经过保存和读回不能丢失。
    fn cookie_roundtrip() {
        let raw = "SESSDATA=a%2Cb==; bili_jct=j; DedeUserID=9; DedeUserID__ckMd5=x; buvid3=three; buvid4=four; b_nut=nut; b_lsid=lsid";
        let jar = CookieJar::decode(raw);
        assert_eq!(jar.encode(), raw);
        assert_eq!(CookieJar::decode(&jar.encode()), jar);
        assert!(CookieJar::decode("SESSDATA=old").is_logged_in());
        assert!(CookieJar::default().header().is_none());
        assert!(!format!("{jar:?}").contains("a%2Cb"));
    }

    #[test]
    /// 不受信回调及 Cookie 字段注入不应被带到主站请求。
    fn rejects_untrusted_callback_and_injected_fields() {
        assert!(!CookieJar::from_login_url("https://evil.test/?SESSDATA=secret").is_logged_in());
        let mut jar = CookieJar::default();
        jar.sessdata = "value; injected=secret".into();
        assert!(jar.header().is_none());
        assert!(CookieJar::from_login_url(
            "https://passport.bilibili.com/?SESSDATA=a;bili_jct=secret"
        )
        .header()
        .is_none());
    }

    #[test]
    /// URL 与响应头的白名单字段均可保留，响应头更新不清空未返回字段。
    fn merge_login_sources() {
        let mut jar = CookieJar::from_login_url(
            "https://passport.bilibili.com/?SESSDATA=old&buvid4=four&b_lsid=lsid#fragment",
        );
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=new%2Ctoken==; Path=/; HttpOnly"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("b_nut=123; Expires=Wed, 21 Oct 2030 07:28:00 GMT"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("unknown=secret; Path=/"),
        );
        jar.absorb(&headers);
        jar.merge(&CookieJar::decode("bili_jct=j"));
        assert_eq!(jar.sessdata, "new%2Ctoken==");
        assert_eq!(jar.buvid4, "four");
        assert_eq!(jar.b_lsid, "lsid");
        assert_eq!(jar.b_nut, "123");
        assert!(!jar.encode().contains("unknown"));
        assert!(!jar.encode().contains("Expires"));
    }
}

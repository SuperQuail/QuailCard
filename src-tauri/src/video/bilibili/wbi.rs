//! B 站 WBI 请求签名：mixin key 重排与 w_rid 计算。
//!
//! 签名算法与会话无关，只依赖 nav 接口返回的 img_key 与 sub_key，
//! 因此可以用固定向量固化行为，避免实现漂移导致接口 403。

use md5::{Digest, Md5};

/// 官方使用的 64 位重排表，取前 32 位作为 mixin key。
const MIXIN_KEY_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// 已解析的 WBI 密钥，按小时缓存后重复使用。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WbiKeys {
    mixin_key: String,
}

impl WbiKeys {
    /// 从 nav 接口的 img_url 与 sub_url 推导 mixin key。
    pub(crate) fn from_urls(img_url: &str, sub_url: &str) -> Self {
        let combined = format!("{}{}", file_stem(img_url), file_stem(sub_url));
        let chars: Vec<char> = combined.chars().collect();
        let key: String = MIXIN_KEY_TAB
            .iter()
            .filter_map(|index| chars.get(*index))
            .take(32)
            .collect();
        Self { mixin_key: key }
    }

    /// nav 密钥缺失或格式异常时立即报告，禁止用硬编码密钥掩盖网络错误。
    pub(crate) fn from_nav(data: &serde_json::Value) -> Result<Self, crate::error::CommandError> {
        let img = data["wbi_img"]["img_url"].as_str().unwrap_or("");
        let sub = data["wbi_img"]["sub_url"].as_str().unwrap_or("");
        if [img, sub].iter().any(|url| {
            let stem = file_stem(url);
            stem.len() != 32 || !stem.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(crate::error::CommandError::new(
                "VIDEO_WBI_KEYS_INVALID",
                "登录校验 /x/web-interface/nav：未返回有效的 WBI 签名密钥，请稍后重试",
            ));
        }
        Ok(Self::from_urls(img, sub))
    }

    /// 测试用构造：直接给定 mixin key。
    #[cfg(test)]
    pub(crate) fn from_mixin_key(mixin_key: &str) -> Self {
        Self {
            mixin_key: mixin_key.to_string(),
        }
    }

    /// 追加 wts 与 w_rid：参数先按 key 排序，签名覆盖完整查询串。
    pub(crate) fn sign(&self, params: &[(String, String)], wts: u64) -> Vec<(String, String)> {
        let mut pairs: Vec<_> = params
            .iter()
            .filter(|(key, _)| key != "wts" && key != "w_rid")
            .map(|(key, value)| {
                (
                    key.clone(),
                    value
                        .chars()
                        .filter(|c| !"!'()*".contains(*c))
                        .collect::<String>(),
                )
            })
            .collect();
        pairs.push(("wts".to_string(), wts.to_string()));
        pairs.sort_by(|left, right| left.0.cmp(&right.0));
        let query = query_string(&pairs);
        let w_rid = md5_hex(format!("{query}{}", self.mixin_key).as_bytes());
        pairs.push(("w_rid".to_string(), w_rid));
        pairs
    }
}

/// 把参数拼成查询串，供 URL 使用。
pub(crate) fn query_string(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// WBI 使用 RFC3986 百分号编码，空格必须是 %20 而不是表单的加号。
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// 取 URL 路径中的文件名（不含扩展名），即 WBI 的 img_key / sub_key。
fn file_stem(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name).to_string()
}

/// 小写十六进制 MD5。
fn md5_hex(bytes: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// nav 缺失或畸形密钥不得回退为静态密钥，合法当前密钥沿用固定签名算法。
    fn validates_nav_keys() {
        assert!(WbiKeys::from_nav(&serde_json::json!({})).is_err());
        assert!(WbiKeys::from_nav(
            &serde_json::json!({"wbi_img":{"img_url":"short.png", "sub_url":"short.png"}})
        )
        .is_err());
        let img = "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png";
        let sub = "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png";
        assert_eq!(
            WbiKeys::from_nav(&serde_json::json!({"wbi_img":{"img_url":img,"sub_url":sub}}))
                .unwrap(),
            WbiKeys::from_urls(img, sub)
        );
    }

    #[test]
    /// 固定向量：官方重排表与 MD5 组合结果不得漂移。
    fn signature_matches_fixed_vector() {
        let keys = WbiKeys::from_urls(
            "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png",
            "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png",
        );
        assert_eq!(keys.mixin_key.len(), 32);
        let signed = keys.sign(
            &[("b".into(), "2".into()), ("a".into(), "1".into())],
            1_700_000_000,
        );
        assert_eq!(signed[0].0, "a");
        assert_eq!(signed.last().unwrap().0, "w_rid");
        assert_eq!(signed.last().unwrap().1, "ea94b54d19b1fed4fc11466aa87c5304");
        assert_eq!(signed[2].0, "wts");
    }

    #[test]
    /// 特殊字符过滤并编码，旧签名参数不能参与新签名。
    fn encodes_and_filters_signature_parameters() {
        let keys = WbiKeys::from_mixin_key("0123456789abcdef0123456789abcdef");
        let signed = keys.sign(
            &[
                ("a b".into(), "中 文&=+!'()*".into()),
                ("wts".into(), "old".into()),
                ("w_rid".into(), "old-secret".into()),
            ],
            42,
        );
        let query = query_string(&signed);
        assert!(query.starts_with("a%20b=%E4%B8%AD%20%E6%96%87%26%3D%2B&wts=42&w_rid="));
        assert!(!query.contains("old"));
        assert_eq!(signed.iter().filter(|(key, _)| key == "wts").count(), 1);
    }

    #[test]
    /// 参数顺序不影响签名结果。
    fn signature_is_order_independent() {
        let keys = WbiKeys::from_mixin_key("0123456789abcdef0123456789abcdef");
        let first = keys.sign(&[("a".into(), "1".into()), ("b".into(), "2".into())], 42);
        let second = keys.sign(&[("b".into(), "2".into()), ("a".into(), "1".into())], 42);
        assert_eq!(first, second);
    }
}

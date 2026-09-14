use serde_json::{json, Value};

use super::{generation_ports::DictionaryLookup, generation_round::error_result};
use crate::dictionary::DictionaryEntry;

const MAX_RESULT_CHARS: usize = 6_000;

/// 工具结果分别携带模型可见内容与首次有效查询的进展键。
pub(super) struct LookupResult {
    pub content: String,
    pub found_words: Vec<String>,
}

/// 严格校验整个参数集合，禁止悄悄丢弃无效词条造成模型误判。
fn words_from_arguments(arguments: &Value) -> Option<Vec<&str>> {
    let object = arguments.as_object()?;
    if object.len() != 1 {
        return None;
    }
    let words = object.get("words")?.as_array()?;
    if words.is_empty() || words.len() > 50 {
        return None;
    }
    words
        .iter()
        .map(|word| {
            let word = word.as_str()?.trim();
            (!word.is_empty() && word.chars().count() <= 200).then_some(word)
        })
        .collect()
}

/// 词典失败变为当前工具错误，截断明确列出未返回的词条。
pub(super) async fn resolve_lookup_result(
    dictionary: &dyn DictionaryLookup,
    arguments: &Value,
) -> LookupResult {
    let Some(words) = words_from_arguments(arguments) else {
        return LookupResult {
            content: error_result(
                "INVALID_SCHEMA",
                "words 必须包含 1-50 个非空词条，每个最多 200 字符",
            ),
            found_words: vec![],
        };
    };
    let mut results = Vec::new();
    let mut found_words = Vec::new();
    let mut omitted_words = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let entry = match dictionary.lookup(word).await {
            Ok(entry) => entry,
            Err(_) => {
                return LookupResult {
                    content: error_result(
                        "DICTIONARY_ERROR",
                        "词典查询暂时不可用，可跳过查询并依据材料继续",
                    ),
                    found_words: vec![],
                }
            }
        };
        let identity = entry
            .as_ref()
            .map(|entry| crate::card_generation::card_identity("vocabulary", "", &entry.word));
        results.push(entry_to_json(word, entry));
        if json!({"results":results}).to_string().chars().count() > MAX_RESULT_CHARS {
            results.pop();
            omitted_words.extend_from_slice(&words[index..]);
            break;
        }
        if let Some(identity) = identity {
            found_words.push(identity);
        }
    }
    LookupResult {
        content: json!({"ok":true,"results":results,"truncated":!omitted_words.is_empty(),"omittedWords":omitted_words}).to_string(),
        found_words,
    }
}

/// 未命中保留占位，避免模型将缺失结果误读为已确认的词义。
fn entry_to_json(word: &str, entry: Option<DictionaryEntry>) -> Value {
    match entry {
        Some(entry) => json!({
            "word":entry.word,"found":true,"phonetic":entry.phonetic,"translation":entry.translation,
            "definition":entry.definition,"pos":entry.pos,"collins":entry.collins,"oxford":entry.oxford,
            "bnc":entry.bnc,"frq":entry.frq,"exchange":entry.exchange
        }),
        None => json!({"word":word,"found":false}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::generation_ports::PortFuture;

    struct OversizedDictionary;
    impl DictionaryLookup for OversizedDictionary {
        /// 巨大首个词条用于验证截断没有悄悄遗漏请求项。
        fn lookup<'a>(&'a self, word: &'a str) -> PortFuture<'a, Option<DictionaryEntry>> {
            Box::pin(async move {
                Ok(Some(DictionaryEntry {
                    word: word.to_string(),
                    phonetic: None,
                    translation: Some("释义".repeat(MAX_RESULT_CHARS)),
                    definition: None,
                    pos: None,
                    collins: None,
                    oxford: None,
                    bnc: None,
                    frq: None,
                    exchange: None,
                }))
            })
        }
    }

    #[tokio::test]
    /// 首个词条本身超限时也必须列出全部未返回词条。
    async fn truncation_lists_every_omitted_word() {
        let result =
            resolve_lookup_result(&OversizedDictionary, &json!({"words":["one","two"]})).await;
        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["truncated"], true);
        assert_eq!(parsed["omittedWords"], json!(["one", "two"]));
        assert_eq!(parsed["results"], json!([]));
        assert!(result.found_words.is_empty());
    }

    #[tokio::test]
    /// 非字符串、超长数组或未知参数都会整体拒绝而非静默截取。
    async fn malformed_lookup_is_rejected_as_one_tool_error() {
        for arguments in [
            json!({"words":["one",42]}),
            json!({"words":vec!["one";51]}),
            json!({"words":["one"],"extra":true}),
        ] {
            let result = resolve_lookup_result(&OversizedDictionary, &arguments).await;
            let parsed: Value = serde_json::from_str(&result.content).unwrap();
            assert_eq!(parsed["error"]["code"], "INVALID_SCHEMA");
        }
    }
}

//! 单词组领域模型的测试：读容忍、版本熔断、结构校验与卡片字段映射。

use super::{
    card_fields, initial_document, parse, search_text, serialize, WordGroupDocument, WordPage,
    FORMAT_VERSION, MAX_EXAMPLE_CHARS, MAX_ID_CHARS, MAX_MEANING_CHARS, MAX_PAGES,
    MAX_PHONETIC_CHARS, MAX_WORD_CHARS,
};

/// 构造一个最小完整页面，测试按需覆盖字段。
fn page(id: &str, word: &str, meaning: &str) -> WordPage {
    WordPage {
        id: id.to_string(),
        word: word.to_string(),
        meaning: meaning.to_string(),
        ..WordPage::default()
    }
}

/// 构造一个只含指定页面的合法文档。
fn document(words: Vec<WordPage>) -> WordGroupDocument {
    WordGroupDocument {
        format_version: FORMAT_VERSION,
        words,
    }
}

#[test]
/// 缺字段取默认、未知字段忽略，缺失 formatVersion 按当前版本读取。
fn parses_tolerantly_with_defaults() {
    let parsed = parse(r#"{"words":[{"id":"a","word":"ephemeral","extra":1}]}"#).unwrap();
    assert_eq!(parsed.format_version, FORMAT_VERSION);
    assert_eq!(parsed.words.len(), 1);
    assert_eq!(parsed.words[0].word, "ephemeral");
    assert!(parsed.words[0].meaning.is_empty());
    assert!(parsed.words[0].phonetic.is_empty());

    let empty = parse(r#"{"formatVersion":1,"futureField":{"x":1}}"#).unwrap();
    assert!(empty.words.is_empty());
}

#[test]
/// 版本高于当前应用时熔断，返回专门错误码。
fn rejects_newer_format_version() {
    let error = parse(r#"{"formatVersion":2,"words":[]}"#).unwrap_err();
    assert_eq!(error.code, "WORD_GROUP_FORMAT_NEWER");
}

#[test]
/// 非法 JSON 与非整数版本都按内容不合法拒绝。
fn rejects_malformed_json_and_version() {
    assert_eq!(parse("{不是 JSON").unwrap_err().code, "WORD_GROUP_INVALID");
    assert_eq!(
        parse(r#"{"formatVersion":"1","words":[]}"#)
            .unwrap_err()
            .code,
        "WORD_GROUP_INVALID"
    );
    assert_eq!(
        parse(r#"{"formatVersion":1,"words":{}}"#).unwrap_err().code,
        "WORD_GROUP_INVALID"
    );
}

#[test]
/// 页数、id 合法性与长度上限逐项校验，错误信息带页码。
fn validates_pages_and_field_limits() {
    let too_many = document(
        (0..=MAX_PAGES)
            .map(|index| page(&format!("p{index}"), "w", "m"))
            .collect(),
    );
    assert_eq!(
        parse(&serialize(&too_many).unwrap()).unwrap_err().code,
        "WORD_GROUP_INVALID"
    );

    for id in [
        String::new(),
        "有中文".to_string(),
        "带 空格".to_string(),
        "a".repeat(MAX_ID_CHARS + 1),
    ] {
        let result = parse(&serialize(&document(vec![page(&id, "w", "m")])).unwrap());
        assert_eq!(result.unwrap_err().code, "WORD_GROUP_INVALID", "id: {id:?}");
    }
    assert!(
        parse(&serialize(&document(vec![page(&"a".repeat(MAX_ID_CHARS), "w", "m")])).unwrap())
            .is_ok()
    );

    let mut long_word = page("a", "w", "m");
    long_word.word = "w".repeat(MAX_WORD_CHARS + 1);
    assert_eq!(
        parse(&serialize(&document(vec![long_word])).unwrap())
            .unwrap_err()
            .code,
        "WORD_GROUP_INVALID"
    );

    let mut long_phonetic = page("a", "w", "m");
    long_phonetic.phonetic = "p".repeat(MAX_PHONETIC_CHARS + 1);
    assert!(parse(&serialize(&document(vec![long_phonetic])).unwrap()).is_err());

    let mut long_meaning = page("a", "w", "m");
    long_meaning.meaning = "m".repeat(MAX_MEANING_CHARS + 1);
    assert!(parse(&serialize(&document(vec![long_meaning])).unwrap()).is_err());

    let mut long_example = page("a", "w", "m");
    long_example.example = "e".repeat(MAX_EXAMPLE_CHARS + 1);
    assert!(parse(&serialize(&document(vec![long_example])).unwrap()).is_err());
}

#[test]
/// 重复 id 会导致卡片 id 冲突，必须拒绝。
fn rejects_duplicate_ids() {
    let duplicated = document(vec![page("same", "a", "甲"), page("same", "b", "乙")]);
    assert_eq!(
        parse(&serialize(&duplicated).unwrap()).unwrap_err().code,
        "WORD_GROUP_INVALID"
    );
}

#[test]
/// 初始文档恰好 1 个空页，id 是合法 uuid v7。
fn builds_initial_document_with_single_page() {
    let document = initial_document();
    assert_eq!(document.format_version, FORMAT_VERSION);
    assert_eq!(document.words.len(), 1);
    let page = &document.words[0];
    assert!(page.word.is_empty() && page.meaning.is_empty());
    let id = uuid::Uuid::parse_str(&page.id).expect("初始页 id 应为合法 UUID");
    assert_eq!(id.get_version_num(), 7);
}

#[test]
/// 完整页映射为卡片字段（front=释义、back=单词），草稿页返回 None。
fn maps_complete_pages_to_card_fields() {
    let mut complete = page("a", " ephemeral ", " 短暂的 ");
    complete.phonetic = " /iˈfemərəl/ ".to_string();
    complete.example = " an ephemeral joy ".to_string();
    let fields = card_fields(&complete).expect("完整页应生成卡片字段");
    assert_eq!(fields.front, "短暂的");
    assert_eq!(fields.back, "ephemeral");
    assert_eq!(fields.detail, "/iˈfemərəl/");
    assert_eq!(fields.example, "an ephemeral joy");

    assert!(card_fields(&page("a", "ephemeral", "   ")).is_none());
    assert!(card_fields(&page("a", "  ", "短暂的")).is_none());
}

#[test]
/// 搜索文本来自单词、释义与例句，音标不参与。
fn derives_search_text_from_three_fields() {
    let mut first = page("a", "ephemeral", "短暂的");
    first.phonetic = "/iˈfemərəl/".to_string();
    first.example = "an ephemeral joy".to_string();
    let text = search_text(&document(vec![first, page("b", "durable", "持久的")]));
    assert!(text.contains("ephemeral"));
    assert!(text.contains("短暂的"));
    assert!(text.contains("an ephemeral joy"));
    assert!(text.contains("durable"));
    assert!(!text.contains("iˈfemərəl"), "音标不进入搜索文本");
    assert_eq!(search_text(&document(Vec::new())), "");
}

#[test]
/// 序列化使用 2 空格缩进与末尾换行，并能被 parse 原样读回。
fn serializes_stable_json_and_roundtrips() {
    let value = document(vec![page("a", "ephemeral", "短暂的")]);
    let text = serialize(&value).unwrap();
    assert!(text.starts_with("{\n  \"formatVersion\": 1,"));
    assert!(text.ends_with("}\n"));
    assert_eq!(parse(&text).unwrap(), value);
}

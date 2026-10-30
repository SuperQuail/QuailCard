use super::frontmatter::*;

/// 构造带 frontmatter 的正文。
fn wrap(yaml: &str, body: &str) -> String {
    format!("---\n{yaml}---\n{body}")
}

#[test]
/// 标准块状 frontmatter 能被拆出来，正文偏移指向第一行正文。
fn splits_block_frontmatter() {
    let content = wrap("tags: [rust]\n", "# 标题\n正文");
    let (yaml, body_start) = split_frontmatter(&content);
    assert_eq!(yaml, Some("tags: [rust]\n"));
    assert_eq!(&content[body_start..], "# 标题\n正文");
}

#[test]
/// 第一行不是围栏时整篇都是正文。
fn ignores_document_without_fence() {
    let content = "# 标题\n---\ntags: [rust]\n";
    assert_eq!(split_frontmatter(content), (None, 0));
}

#[test]
/// 没有闭合围栏时不把后续正文误吞成 YAML。
fn rejects_unterminated_frontmatter() {
    let content = "---\ntags: [rust]\n# 标题\n正文";
    assert_eq!(split_frontmatter(content), (None, 0));
}

#[test]
/// YAML 的另一种闭合符 ... 同样被接受。
fn accepts_dot_close_fence() {
    let content = "---\ntags: [rust]\n...\n正文";
    let (yaml, body_start) = split_frontmatter(content);
    assert_eq!(yaml, Some("tags: [rust]\n"));
    assert_eq!(&content[body_start..], "正文");
}

#[test]
/// CRLF 换行不影响判定与偏移。
fn handles_crlf() {
    let content = "---\r\ntags: [rust]\r\n---\r\n正文";
    let (yaml, body_start) = split_frontmatter(content);
    assert_eq!(yaml, Some("tags: [rust]\r\n"));
    assert_eq!(&content[body_start..], "正文");
}

#[test]
/// UTF-8 BOM 不影响识别，且正文偏移跳过 BOM。
fn handles_bom() {
    let content = "\u{feff}---\ntags: [rust]\n---\n正文";
    let (yaml, body_start) = split_frontmatter(content);
    assert_eq!(yaml, Some("tags: [rust]\n"));
    assert_eq!(&content[body_start..], "正文");
}

#[test]
/// 超出最大行数上限的头部不再当作 frontmatter。
fn rejects_overlong_frontmatter() {
    let filler = "key: value\n".repeat(MAX_FRONTMATTER_LINES + 1);
    let content = format!("---\n{filler}---\n正文");
    assert_eq!(split_frontmatter(&content), (None, 0));
}

#[test]
/// 数组形式、逗号字符串与单值写法都能读出标签。
fn reads_tags_in_all_shapes() {
    let block = parse_meta("tags:\n  - rust\n  - 所有权\n");
    assert_eq!(block.tags, ["rust", "所有权"]);
    let flow = parse_meta("tags: [rust, 所有权]\n");
    assert_eq!(flow.tags, ["rust", "所有权"]);
    let list = parse_meta("tags: rust, 所有权\n");
    assert_eq!(list.tags, ["rust", "所有权"]);
    let single = parse_meta("tags: rust\n");
    assert_eq!(single.tags, ["rust"]);
}

#[test]
/// 标签里的前导 # 会被去掉，重复项只保留一次。
fn normalizes_tag_prefixes_and_duplicates() {
    let meta = parse_meta("tags: ['#rust', rust, '#rust']\n");
    assert_eq!(meta.tags, ["rust"]);
}

#[test]
/// 别名支持列表与单值两种写法。
fn reads_aliases() {
    let meta = parse_meta("aliases: [Bee, 小蜜蜂]\n");
    assert_eq!(meta.aliases, ["Bee", "小蜜蜂"]);
    let single = parse_meta("aliases: Bee\n");
    assert_eq!(single.aliases, ["Bee"]);
}

#[test]
/// created 原样保留，不做时间格式猜测。
fn keeps_created_verbatim() {
    let meta = parse_meta("created: 2024-01-02\n");
    assert_eq!(meta.created.as_deref(), Some("2024-01-02"));
}

#[test]
/// 未知字段被忽略，不影响已知字段。
fn ignores_unknown_keys() {
    let meta = parse_meta("title: 别的标题\ncustom: 1\ntags: [rust]\n");
    assert_eq!(meta.tags, ["rust"]);
}

#[test]
/// 非法 YAML 返回空元数据而不是报错。
fn tolerates_broken_yaml() {
    let meta = parse_meta("tags: [unclosed\n  : :\n");
    assert_eq!(meta, NoteMeta::default());
}

#[test]
/// 非映射的 YAML 文档同样按无元数据处理。
fn ignores_non_mapping_document() {
    assert_eq!(parse_meta("- 1\n- 2\n"), NoteMeta::default());
    assert_eq!(parse_meta(""), NoteMeta::default());
}

#[test]
/// parse_content 一次给出元数据与已剥离的头部的正文。
fn parses_meta_and_body_together() {
    let content = wrap(
        "tags: [rust]\naliases: [R]\ncreated: 2024-01-02\n",
        "正文\n#标签",
    );
    let (meta, body) = parse_content(&content);
    assert_eq!(meta.tags, ["rust"]);
    assert_eq!(meta.aliases, ["R"]);
    assert_eq!(meta.created.as_deref(), Some("2024-01-02"));
    assert_eq!(body, "正文\n#标签");
}

#[test]
/// 没有 frontmatter 时元数据为空且正文完整。
fn returns_empty_meta_without_frontmatter() {
    let (meta, body) = parse_content("# 标题\n正文");
    assert_eq!(meta, NoteMeta::default());
    assert_eq!(body, "# 标题\n正文");
}

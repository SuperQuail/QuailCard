//! 双链扫描、解析与改写的契约测试。

use super::links::*;

/// 与前端共用的解析夹具；解析规则改动必须先改夹具。
#[derive(serde::Deserialize)]
struct Fixture {
    notes: Vec<FixtureNote>,
    cases: Vec<FixtureCase>,
}

#[derive(serde::Deserialize)]
struct FixtureNote {
    path: String,
    title: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(serde::Deserialize)]
struct FixtureCase {
    name: String,
    from: String,
    target: String,
    expect: Option<String>,
}

/// 读取前后端共用夹具：前端经 Vite 引用，后端从仓库根目录读取。
fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/domain/linkResolution.fixture.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取链接解析夹具失败 {path:?}: {error}"));
    serde_json::from_str(&raw).expect("链接解析夹具格式无效")
}

/// 计算字节位置之前的 UTF-16 长度，用于在测试中表达期望偏移。
fn utf16_len_before(text: &str, byte: usize) -> usize {
    text[..byte].encode_utf16().count()
}

#[test]
/// 夹具中的每条解析用例都必须与前端给出完全相同的结果。
fn resolve_matches_shared_fixture() {
    let fixture = fixture();
    let notes: Vec<ResolveNote<'_>> = fixture
        .notes
        .iter()
        .map(|note| ResolveNote {
            path: &note.path,
            title: &note.title,
            aliases: &note.aliases,
        })
        .collect();
    for case in &fixture.cases {
        assert_eq!(
            resolve(&case.target, &case.from, &notes),
            case.expect,
            "夹具用例未通过：{}",
            case.name
        );
    }
}

#[test]
/// 扫描识别标题与显示文本，跳过围栏代码块与行内代码，嵌入写法含 ! 前缀。
fn scans_links_and_skips_code() {
    let body = "开头 [[目标]] 与 [[目标#小节|显示]]\n~~~\n[[围栏内]]\n~~~\n行内 \u{60}[[行内代码]]\u{60} 结束 ![[图片]] [[空|]]";
    let links = scan_links(body);
    let targets: Vec<&str> = links.iter().map(|link| link.target.as_str()).collect();
    assert_eq!(targets, ["目标", "目标", "图片", "空"]);
    assert_eq!(links[1].heading.as_deref(), Some("小节"));
    assert_eq!(links[1].label.as_deref(), Some("显示"));
    let embed_start = utf16_to_byte(body, links[2].from).expect("偏移必须有效");
    assert!(body[embed_start..].starts_with("![[图片]]"));
}

#[test]
/// 偏移按 UTF-16 计算：emoji 占两个单元，中文与 emoji 混合时不能错位。
fn reports_utf16_offsets() {
    let body = "😀中文 [[目标]] 尾巴";
    let links = scan_links(body);
    assert_eq!(links.len(), 1);
    let open = body.find("[[").expect("应能定位链接");
    assert_eq!(links[0].from, utf16_len_before(body, open));
    assert_eq!(
        links[0].to,
        utf16_len_before(body, open) + "[[目标]]".encode_utf16().count()
    );
    assert_eq!(utf16_to_byte(body, links[0].from), Some(open));
}

#[test]
/// 跨行的 [[ 不算链接，避免把大段正文误判成一条链接。
fn ignores_links_across_lines() {
    let body = "开头 [[目标\n下一行]] 结尾";
    assert!(scan_links(body).is_empty());
}

#[test]
/// 空目标与只有标题的写法都不算链接。
fn ignores_empty_targets() {
    assert!(scan_links("[[]] [[#仅标题]] [[]] ").is_empty());
}

#[test]
/// 改写只替换目标文本：标题、显示文本、嵌入前缀、frontmatter 与代码保持原样。
fn rewrites_only_target_text() {
    let content =
        "---\nnote: [[旧]]\n---\n看 [[旧#小节|显示]] 和 ![[旧]]，行内 \u{60}[[旧]]\u{60} 不动";
    let links = scan_content_links(content);
    assert_eq!(links.len(), 2, "frontmatter 与行内代码里的链接不算");
    let replacements: Vec<(RawLink, String)> = links
        .iter()
        .map(|link| (link.clone(), "新目录/新名".to_string()))
        .collect();
    let (rewritten, edits) = rewrite_links(content, &replacements);
    assert!(rewritten.contains("看 [[新目录/新名#小节|显示]] 和 ![[新目录/新名]]"));
    assert!(rewritten.contains("note: [[旧]]"), "frontmatter 不能被改写");
    assert!(
        rewritten.contains("\u{60}[[旧]]\u{60}"),
        "行内代码不能被改写"
    );
    assert_eq!(edits.len(), 2);
    assert!(edits[0].to <= edits[1].from, "平移量必须升序且不重叠");
    assert_eq!(edits[0].new_len, "新目录/新名".encode_utf16().count());
    let open = content.find("[[旧#").expect("正文里应存在第一个链接");
    // Edit 指向被替换的目标文本（[[ 之后），而不是整条链接的起点。
    assert_eq!(edits[0].from, utf16_len_before(content, open) + 2);
    assert_eq!(edits[0].to, edits[0].from + "旧".encode_utf16().count());
}

#[test]
/// 没有命中链接时内容原样返回，不产生任何平移量。
fn rewrite_without_matches_keeps_content() {
    let content = "纯正文，没有双链";
    let (rewritten, edits) = rewrite_links(content, &[]);
    assert_eq!(rewritten, content);
    assert!(edits.is_empty());
}

#[test]
/// 改名后目标变短也能正确改写，后续锚点按负增量平移。
fn rewrites_shorter_target() {
    let content = "甲 [[很长很长的名字]] 乙";
    let links = scan_links(content);
    let replacements: Vec<(RawLink, String)> = links
        .iter()
        .map(|link| (link.clone(), "短".to_string()))
        .collect();
    let (rewritten, edits) = rewrite_links(content, &replacements);
    assert_eq!(rewritten, "甲 [[短]] 乙");
    assert_eq!(edits.len(), 1);
    assert!(edits[0].new_len < edits[0].to - edits[0].from);
}

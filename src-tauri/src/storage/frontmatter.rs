//! frontmatter（YAML 头）的容错只读解析。
//!
//! 应用从不写回 frontmatter：这里只把用户或外部工具写入的元数据读出来，
//! 供标签、别名与创建时间使用。遵守文件格式四律的第一条（读容忍）：
//! 缺字段取默认、未知字段忽略、解析失败既不上报也不阻断 Vault 扫描——
//! 元数据坏掉不该让笔记正文无法打开。

use yaml_rust2::{yaml::Hash, Yaml, YamlLoader};

/// frontmatter 最多允许的行数：超过即认为不是元数据头，避免把正文误吞。
pub(crate) const MAX_FRONTMATTER_LINES: usize = 200;

/// 单个笔记的元数据；三项都是可选的，缺失即空。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct NoteMeta {
    /// frontmatter 声明的标签，已去掉前导 #。
    pub(crate) tags: Vec<String>,
    /// 别名：双链解析时与标题同等对待。
    pub(crate) aliases: Vec<String>,
    /// 创建时间，原样保留字符串（不解析成时间，避免格式猜测出错）。
    pub(crate) created: Option<String>,
}

/// 判断是否是 frontmatter 的起始围栏；行尾空白与换行符不计。
fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
}

/// 判断是否是 frontmatter 的结束围栏；YAML 允许 `---` 或 `...` 收尾。
fn is_close(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed == "---" || trimmed == "..."
}

/// 拆分正文与 frontmatter，返回 (YAML 文本, 正文起始字节偏移)。
///
/// 契约：只有文档第一行恰为 `---` 且在前 [MAX_FRONTMATTER_LINES] 行内找到结束围栏时才成立；
/// 否则返回 (None, 0)，调用方应把整篇当正文。UTF-8 BOM 不影响判定，
/// 但会在返回的正文偏移里被跳过。
pub(crate) fn split_frontmatter(content: &str) -> (Option<&str>, usize) {
    // 去掉 BOM 后按行处理；base 用于把偏移换算回原文坐标。
    let (text, base) = match content.strip_prefix('\u{feff}') {
        Some(rest) => (rest, content.len() - rest.len()),
        None => (content, 0),
    };
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (None, 0);
    };
    if !is_fence(first) {
        return (None, 0);
    }
    let yaml_start = first.len();
    let mut offset = yaml_start;
    for (index, line) in lines.enumerate() {
        if index >= MAX_FRONTMATTER_LINES {
            return (None, 0);
        }
        let end = offset + line.len();
        if is_close(line) {
            return (Some(&text[yaml_start..offset]), base + end);
        }
        offset = end;
    }
    // 没有闭合围栏：整篇按正文处理，绝不吞掉后面的内容。
    (None, 0)
}

/// 从 YAML 文本解析元数据；解析失败只记日志并返回空元数据。
pub(crate) fn parse_meta(yaml: &str) -> NoteMeta {
    let documents = match YamlLoader::load_from_str(yaml) {
        Ok(documents) => documents,
        Err(error) => {
            eprintln!("frontmatter 解析失败，按无元数据处理：{error}");
            return NoteMeta::default();
        }
    };
    let Some(Yaml::Hash(map)) = documents.first() else {
        return NoteMeta::default();
    };
    NoteMeta {
        tags: list_values(map, "tags"),
        aliases: list_values(map, "aliases"),
        created: scalar_value(map, "created"),
    }
}

/// 一步取出正文与元数据：正文已经剥离 frontmatter 与 BOM。
pub(crate) fn parse_content(content: &str) -> (NoteMeta, &str) {
    let (yaml, body_start) = split_frontmatter(content);
    let meta = yaml.map(parse_meta).unwrap_or_default();
    let start = body_start.min(content.len());
    (meta, &content[start..])
}

/// 在映射里按键取值；键名大小写敏感（与 YAML 一致）。
fn lookup<'a>(map: &'a Hash, key: &str) -> Option<&'a Yaml> {
    map.get(&Yaml::String(key.to_string()))
}

/// 把标量节点转成字符串；非标量（映射/数组）一律忽略。
fn scalar_of(node: &Yaml) -> Option<String> {
    match node {
        Yaml::String(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Real(text) => Some(text.clone()),
        Yaml::Boolean(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// 读取单个标量字段。
fn scalar_value(map: &Hash, key: &str) -> Option<String> {
    lookup(map, key)
        .and_then(scalar_of)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 读取列表字段：支持 YAML 数组、逗号/空白分隔字符串与单值。
fn list_values(map: &Hash, key: &str) -> Vec<String> {
    let Some(node) = lookup(map, key) else {
        return Vec::new();
    };
    let raw: Vec<String> = match node {
        Yaml::Array(items) => items.iter().filter_map(scalar_of).collect(),
        Yaml::String(text) => text
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::to_string)
            .collect(),
        other => scalar_of(other).into_iter().collect(),
    };
    normalize_tags(raw)
}

/// 归一化标签/别名：去掉前导 # 与首尾空白、丢弃空项、按出现顺序去重。
fn normalize_tags(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let cleaned = value.trim().trim_start_matches('#').trim().to_string();
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            result.push(cleaned);
        }
    }
    result
}

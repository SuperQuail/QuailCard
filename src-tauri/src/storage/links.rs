//! 双链（[[...]]）的扫描、解析与改写。
//!
//! 与前端 src/domain/linkResolution.fixture.json 契约一致：解析规则的任何改动
//! 都必须先改夹具。偏移统一使用 UTF-16，与编辑器 CardSource 坐标系相同，
//! 卡片来源锚点可以直接按改写量平移。

use super::frontmatter;

/// 正文中的一处双链；from/to 覆盖整个 [[...]]（含 !），UTF-16 偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawLink {
    /// 目标文本（已去首尾空白，不含 #标题 与 |显示）。
    pub(crate) target: String,
    /// # 之后的标题片段；解析时忽略，保留给跳转定位。
    #[allow(dead_code)]
    pub(crate) heading: Option<String>,
    /// | 之后的显示文本；解析时忽略，保留给渲染。
    #[allow(dead_code)]
    pub(crate) label: Option<String>,
    /// 链接起点（UTF-16，嵌入写法含 ! 前缀）。
    pub(crate) from: usize,
    /// 链接终点（UTF-16，位于 ]] 之后）。
    pub(crate) to: usize,
}

/// 一次链接改写造成的文本平移，供卡片来源锚点跟随。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Edit {
    /// 被替换区间的起点（UTF-16，是目标文本而非整条链接）。
    pub(crate) from: usize,
    /// 被替换区间的终点（UTF-16）。
    pub(crate) to: usize,
    /// 替换后文本的 UTF-16 长度。
    pub(crate) new_len: usize,
}

/// 解析链接目标时可见的一篇笔记。
pub(crate) struct ResolveNote<'a> {
    /// Vault 相对路径，例如 dir/B.md。
    pub(crate) path: &'a str,
    /// 显示标题（文件名去掉 .md）。
    pub(crate) title: &'a str,
    /// frontmatter 声明的别名。
    pub(crate) aliases: &'a [String],
}

/// 扫描正文中的双链；围栏代码块与行内代码内的内容不算。
///
/// 偏移以 body 起点为 0 按 UTF-16 计算；扫整篇笔记请用 scan_content_links。
pub(crate) fn scan_links(body: &str) -> Vec<RawLink> {
    let chars: Vec<char> = body.chars().collect();
    let mask = code_mask(&chars);
    let offsets = utf16_offsets(&chars);
    let mut links = Vec::new();
    let mut index = 0;
    while index + 1 < chars.len() {
        if chars[index] != '[' || chars[index + 1] != '[' || mask[index] || mask[index + 1] {
            index += 1;
            continue;
        }
        let Some(close) = find_close(&chars, &mask, index + 2) else {
            index += 2;
            continue;
        };
        let inner: String = chars[index + 2..close].iter().collect();
        let start = if index > 0 && chars[index - 1] == '!' && !mask[index - 1] {
            index - 1
        } else {
            index
        };
        if let Some(link) = build_link(&inner, offsets[start], offsets[close + 2]) {
            links.push(link);
        }
        index = close + 2;
    }
    links
}

/// 扫描整篇笔记正文中的双链，自动跳过 frontmatter 并换算到文档坐标。
pub(crate) fn scan_content_links(content: &str) -> Vec<RawLink> {
    let (_, body_start) = frontmatter::split_frontmatter(content);
    let start = body_start.min(content.len());
    let mut links = scan_links(&content[start..]);
    let base = content[..start].encode_utf16().count();
    for link in &mut links {
        link.from += base;
        link.to += base;
    }
    links
}

/// 找到与 [[ 配对的 ]]；跨行或落在代码里的不算。
fn find_close(chars: &[char], mask: &[bool], start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor + 1 < chars.len() {
        if chars[cursor] == '\n' {
            return None;
        }
        if chars[cursor] == ']' && chars[cursor + 1] == ']' && !mask[cursor] && !mask[cursor + 1] {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

/// 解析 [[目标#标题|显示]] 的内部文本；空目标不算链接。
fn build_link(inner: &str, from: usize, to: usize) -> Option<RawLink> {
    let (body, label) = match inner.split_once('|') {
        Some((body, label)) => (body, non_empty(label.trim())),
        None => (inner, None),
    };
    let (target, heading) = match body.split_once('#') {
        Some((target, heading)) => (target, non_empty(heading.trim())),
        None => (body, None),
    };
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    Some(RawLink {
        target: target.to_string(),
        heading,
        label,
        from,
        to,
    })
}

/// 把空串折叠成 None，避免调用方区分“缺失”与“空”。
fn non_empty(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 解析双链目标为 Vault 相对路径；规则顺序与前端实现完全一致。
///
/// 顺序：
/// 1) Vault 相对路径精确匹配（去 .md、统一斜杠、大小写不敏感）；
/// 2) 相对来源笔记目录匹配；
/// 3) 标题匹配；
/// 4) 别名匹配。
///
/// 3、4 的多个候选按“同目录 → 路径更短 → 字典序”取第一个；都没有返回 None。
pub(crate) fn resolve(target: &str, from_path: &str, notes: &[ResolveNote<'_>]) -> Option<String> {
    let wanted = normalize_key(target);
    if wanted.is_empty() {
        return None;
    }
    if let Some(note) = notes.iter().find(|note| normalize_key(note.path) == wanted) {
        return Some(note.path.to_string());
    }
    let from_dir = directory_of(from_path);
    if !from_dir.is_empty() {
        let relative = format!("{from_dir}/{wanted}");
        if let Some(note) = notes
            .iter()
            .find(|note| normalize_key(note.path) == relative)
        {
            return Some(note.path.to_string());
        }
    }
    if let Some(note) = pick(
        notes
            .iter()
            .filter(|note| normalize_key(note.title) == wanted),
        &from_dir,
    ) {
        return Some(note.path.to_string());
    }
    if let Some(note) = pick(
        notes.iter().filter(|note| {
            note.aliases
                .iter()
                .any(|alias| normalize_key(alias) == wanted)
        }),
        &from_dir,
    ) {
        return Some(note.path.to_string());
    }
    None
}

/// 多个同名候选时按“同目录 → 路径更短 → 字典序”取第一个。
fn pick<'a, 'b>(
    candidates: impl Iterator<Item = &'b ResolveNote<'a>>,
    from_dir: &str,
) -> Option<&'b ResolveNote<'a>> {
    candidates.min_by(|left, right| {
        let left_same = directory_of(left.path) == from_dir;
        let right_same = directory_of(right.path) == from_dir;
        (!left_same)
            .cmp(&(!right_same))
            .then_with(|| left.path.chars().count().cmp(&right.path.chars().count()))
            .then_with(|| left.path.cmp(right.path))
    })
}

/// 与前端 normalize 同序：trim → 反斜杠转斜杠 → 去前导 ./ → 去 .md →
/// 去尾部斜杠 → 小写。两端必须同序，夹具才能同时通过。
pub(crate) fn normalize_key(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    let stripped = trimmed.strip_prefix("./").unwrap_or(&trimmed).to_string();
    let lower = stripped.to_lowercase();
    let without_extension = lower.strip_suffix(".md").unwrap_or(&lower).to_string();
    without_extension.trim_end_matches('/').to_string()
}

/// 取规范化路径最后一段之前的目录；根目录笔记返回空串。
pub(crate) fn directory_of(path: &str) -> String {
    let normalized = normalize_key(path);
    match normalized.rsplit_once('/') {
        Some((directory, _)) => directory.to_string(),
        None => String::new(),
    }
}

/// 改写命中链接的目标文本，返回新内容与全部平移量。
///
/// matches 每项给出该链接的新写法；#标题、|显示 与 ! 前缀原样保留。
/// 定位失败或区间重叠的项直接跳过：宁可少改，也不破坏正文。
pub(crate) fn rewrite_links(content: &str, matches: &[(RawLink, String)]) -> (String, Vec<Edit>) {
    let mut ordered: Vec<&(RawLink, String)> = matches
        .iter()
        .filter(|(_, replacement)| !replacement.is_empty())
        .collect();
    ordered.sort_by_key(|(link, _)| link.from);
    if ordered.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let mut output = String::with_capacity(content.len());
    let mut copied = 0;
    let mut edits = Vec::with_capacity(ordered.len());
    for (link, replacement) in ordered {
        let Some((from, to)) = target_span(content, link) else {
            continue;
        };
        let (Some(from_byte), Some(to_byte)) =
            (utf16_to_byte(content, from), utf16_to_byte(content, to))
        else {
            continue;
        };
        if from_byte < copied || to_byte < from_byte {
            continue;
        }
        output.push_str(&content[copied..from_byte]);
        output.push_str(replacement);
        copied = to_byte;
        edits.push(Edit {
            from,
            to,
            new_len: replacement.encode_utf16().count(),
        });
    }
    output.push_str(&content[copied..]);
    (output, edits)
}

/// 计算目标文本在整篇内容中的 UTF-16 区间（不含 #标题 与 |显示）。
pub(crate) fn target_span(content: &str, link: &RawLink) -> Option<(usize, usize)> {
    let from_byte = utf16_to_byte(content, link.from)?;
    let to_byte = utf16_to_byte(content, link.to)?;
    let slice = content.get(from_byte..to_byte)?;
    let open = slice.find("[[")?;
    let target_start = open + 2;
    let rest = &slice[target_start..];
    let target_len = rest
        .find(['#', '|'])
        .unwrap_or_else(|| rest.len().saturating_sub(2));
    let prefix = slice[..target_start].encode_utf16().count();
    let target = rest[..target_len].encode_utf16().count();
    Some((link.from + prefix, link.from + prefix + target))
}

/// 把 UTF-16 偏移换算成 UTF-8 字节偏移；越界或落在代理对中间返回 None。
pub(crate) fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
    let mut consumed = 0;
    for (byte, character) in text.char_indices() {
        if consumed == offset {
            return Some(byte);
        }
        consumed += character.len_utf16();
        if consumed > offset {
            return None;
        }
    }
    if consumed == offset {
        Some(text.len())
    } else {
        None
    }
}

/// 预先计算每个字符的 UTF-16 起始偏移，末尾多一项作为整体长度哨兵。
fn utf16_offsets(chars: &[char]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(chars.len() + 1);
    let mut offset = 0;
    for character in chars {
        offsets.push(offset);
        offset += character.len_utf16();
    }
    offsets.push(offset);
    offsets
}

/// 标记哪些字符属于代码（围栏代码块或行内代码），索引与 chars 对齐。
fn code_mask(chars: &[char]) -> Vec<bool> {
    let mut mask = vec![false; chars.len()];
    let mut fence: Option<(char, usize)> = None;
    let mut line_start = 0;
    while line_start < chars.len() {
        let mut line_end = line_start;
        while line_end < chars.len() && chars[line_end] != '\n' {
            line_end += 1;
        }
        let mut cursor = line_start;
        while cursor < line_end && (chars[cursor] == ' ' || chars[cursor] == '\t') {
            cursor += 1;
        }
        let marker = chars
            .get(cursor)
            .copied()
            .filter(|value| *value == '\u{60}' || *value == '~');
        let run = marker.map_or(0, |value| {
            let mut end = cursor;
            while end < line_end && chars[end] == value {
                end += 1;
            }
            end - cursor
        });
        match fence {
            Some((open, length)) => {
                for slot in &mut mask[line_start..line_end] {
                    *slot = true;
                }
                if marker == Some(open) && run >= length {
                    fence = None;
                }
            }
            None if run >= 3 => {
                fence = marker.map(|value| (value, run));
                for slot in &mut mask[line_start..line_end] {
                    *slot = true;
                }
            }
            None => mark_inline_code(chars, &mut mask, line_start, line_end),
        }
        line_start = line_end + 1;
    }
    mask
}

/// 标记一行内的反引号代码片段；不配对的单个反引号按普通字符处理。
fn mark_inline_code(chars: &[char], mask: &mut [bool], start: usize, end: usize) {
    let mut cursor = start;
    while cursor < end {
        if chars[cursor] != '\u{60}' {
            cursor += 1;
            continue;
        }
        let mut run_end = cursor;
        while run_end < end && chars[run_end] == '\u{60}' {
            run_end += 1;
        }
        let run = run_end - cursor;
        let mut search = run_end;
        let mut closing = None;
        while search < end {
            if chars[search] == '\u{60}' {
                let mut close_end = search;
                while close_end < end && chars[close_end] == '\u{60}' {
                    close_end += 1;
                }
                if close_end - search == run {
                    closing = Some(close_end);
                    break;
                }
                search = close_end;
            } else {
                search += 1;
            }
        }
        match closing {
            Some(close_end) => {
                for slot in &mut mask[cursor..close_end] {
                    *slot = true;
                }
                cursor = close_end;
            }
            None => cursor = run_end,
        }
    }
}

//! 生成工具只能读取准备阶段冻结的材料，行号不受磁盘后续编辑影响。
use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Window {
    offset: Option<usize>,
    limit: Option<usize>,
}

/// 返回原始行文本；仅首次读到的行算进展，改变分页大小不能伪造进展。
pub(super) fn read_material(
    material: &str,
    arguments: &Value,
    seen: &mut HashSet<String>,
) -> Result<(Value, bool), &'static str> {
    let window: Window = serde_json::from_value(arguments.clone())
        .map_err(|_| "读取材料参数必须是正整数 offset 与 limit")?;
    let offset = window.offset.unwrap_or(1);
    let limit = window.limit.unwrap_or(200);
    if offset == 0 || !(1..=2000).contains(&limit) {
        return Err("offset 必须从 1 开始，limit 必须在 1–2000 之间");
    }
    let lines: Vec<_> = material.split('\n').collect();
    if offset > lines.len() {
        return Err("offset 超出材料行数，请使用返回的 nextOffset；null 表示已到结尾");
    }
    let start = offset - 1;
    let end = start.saturating_add(limit).min(lines.len());
    let mut progressed = false;
    let window: Vec<_> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let number = start + index + 1;
            progressed |= seen.insert(format!("\0material-line:{number}"));
            json!({"number": number, "text": text})
        })
        .collect();
    Ok((
        json!({
            "ok": true, "offset": offset, "totalLines": lines.len(),
            "lines": window, "nextOffset": (end < lines.len()).then_some(end + 1),
            "instruction": "这是本次生成的固定材料快照；sourceRange 使用这里的行号，不要复制行号或代码正文。"
        }),
        progressed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 代码缩进、空行、围栏与尾空行都必须与用于定位的快照一致。
    fn preserves_code_and_counts_only_new_lines() {
        let material = "```gdscript\n\tif x > 0:\n\t\tdirection = -1\n\n```\n";
        let mut seen = HashSet::new();
        let (first, progress) = read_material(material, &json!({"limit": 3}), &mut seen).unwrap();
        assert!(progress);
        assert_eq!(first["lines"][1], json!({"number":2,"text":"\tif x > 0:"}));
        assert_eq!(first["nextOffset"], 4);
        let (_, progress) =
            read_material(material, &json!({"offset":2,"limit":2}), &mut seen).unwrap();
        assert!(!progress);
        let (last, progress) = read_material(material, &json!({"offset":4}), &mut seen).unwrap();
        assert!(progress);
        assert_eq!(last["lines"][0]["text"], "");
        assert_eq!(last["totalLines"], 6);
        assert!(last["nextOffset"].is_null());
        let (_, progress) = read_material(material, &json!({}), &mut seen).unwrap();
        assert!(!progress);
    }

    #[test]
    /// 越界与无效窗口不给进展，防止失败读取延长重试。
    fn rejects_invalid_windows() {
        let mut seen = HashSet::new();
        for args in [
            json!({"offset":0}),
            json!({"offset":3}),
            json!({"limit":0}),
            json!({"limit":2001}),
            json!({"offset":-1}),
        ] {
            assert!(read_material("a\nb", &args, &mut seen).is_err());
        }
        assert!(seen.is_empty());
    }
}

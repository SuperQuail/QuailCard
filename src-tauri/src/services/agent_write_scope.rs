//! 子代理写入范围：解析、按分段匹配与子集校验。
//!
//! 契约（安全边界，改动前先读）：
//! - 空范围 = 只读；根会话的空范围表示整库。子会话的空范围绝不能被解释成整库。
//! - 以 `/` 结尾的元素是目录前缀（允许在其中 create_note），单独 `/` 表示知识库根目录；
//!   其余元素是精确笔记文件（只允许 edit_note，不允许新建）。
//! - 匹配按路径分段比较，禁止字符串前缀漏洞：`a/` 不覆盖 `ab/x.md`。
//! - 子范围必须是父范围的子集：任何一项超出父范围都整体拒绝，绝不静默缩小或放大。

use crate::error::CommandError;

/// 允许授予的范围项数量上限；超过即有滥用或模型幻觉。
const MAX_ENTRIES: usize = 50;
/// 单项路径长度上限；范围键要进入会话文件，不能让历史无限膨胀。
const MAX_ENTRY_LEN: usize = 512;

/// 本会话的笔记写入授权。
#[derive(Clone, Copy)]
pub(crate) enum WriteAuthority<'a> {
    /// 根会话：整库写权由用户明确交互决定。
    Root,
    /// 子会话：只拥有父级显式授予的路径集合，空集合表示只读。
    Scoped(&'a [String]),
}

impl<'a> WriteAuthority<'a> {
    /// 新建笔记只允许落在目录前缀内：精确文件授权不包含创建权。
    pub(crate) fn check_create(&self, path: &str) -> Result<(), CommandError> {
        let allowed = match self {
            Self::Root => true,
            Self::Scoped(scope) => {
                is_note(path) && scope.iter().any(|entry| covers_dir(entry, path))
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(denied())
        }
    }

    /// 修改已有笔记：精确文件或目录前缀内都可以。
    pub(crate) fn check_edit(&self, path: &str) -> Result<(), CommandError> {
        let allowed = match self {
            Self::Root => true,
            Self::Scoped(scope) => {
                is_note(path) && scope.iter().any(|entry| covers_path(entry, path))
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(denied())
        }
    }
}

/// 规范化后的范围项；目录内部不带结尾 `/`，根目录前缀为空串。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScopeEntry {
    Dir(String),
    File(String),
}

impl ScopeEntry {
    /// 落盘与状态输出使用的规范形。
    pub(crate) fn canonical(&self) -> String {
        match self {
            Self::Dir(prefix) if prefix.is_empty() => "/".into(),
            Self::Dir(prefix) => format!("{prefix}/"),
            Self::File(path) => path.clone(),
        }
    }

    /// 父项是否覆盖子项；空目录前缀（根目录）覆盖整库。
    fn covers(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Dir(prefix), Self::Dir(child)) | (Self::Dir(prefix), Self::File(child)) => {
                let parent = segments(prefix);
                parent.is_empty() || segments(child).starts_with(&parent)
            }
            // 精确文件只覆盖它自己，不能覆盖任何目录前缀。
            (Self::File(path), Self::File(child)) => segments(path) == segments(child),
            (Self::File(_), Self::Dir(_)) => false,
        }
    }
}

/// 解析父级提交的范围：净化 + 规范化，非法项整体拒绝。
pub(crate) fn parse(scope: &[String]) -> Result<Vec<ScopeEntry>, CommandError> {
    if scope.len() > MAX_ENTRIES {
        return Err(scope_denied());
    }
    scope.iter().map(|raw| parse_entry(raw)).collect()
}

/// 计算可落盘的子范围：根是整库，子节点必须逐项落在父范围内。
///
/// 空请求表示只读，对任何父级都合法；非空请求只要有一项越权就整体失败，
/// 不返回部分授权，避免父级误以为子代理拥有它没有的权限。
pub(crate) fn grant(
    requested: &[ScopeEntry],
    parent: &[ScopeEntry],
    parent_unlimited: bool,
) -> Result<Vec<String>, CommandError> {
    if requested.is_empty() {
        return Ok(Vec::new());
    }
    if !parent_unlimited {
        if parent.is_empty() {
            return Err(scope_denied());
        }
        if requested
            .iter()
            .any(|item| !parent.iter().any(|allowed| allowed.covers(item)))
        {
            return Err(scope_denied());
        }
    }
    Ok(canonical(requested))
}

/// 已解析范围的规范形；调用方用它比较磁盘记录与树内授权是否一致。
pub(crate) fn canonical(entries: &[ScopeEntry]) -> Vec<String> {
    canonical_list(entries)
}

/// 给模型与用户看的范围描述；只读时明确说明没有写权限。
pub(crate) fn describe(scope: &[String]) -> String {
    if scope.is_empty() {
        return "无（只读）".into();
    }
    scope
        .iter()
        .map(|entry| {
            if entry == "/" {
                "知识库根目录".to_string()
            } else {
                entry.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("、")
}

/// 越权写入的稳定错误码；消息不包含范围之外的信息。
fn denied() -> CommandError {
    CommandError::new(
        "AGENT_WRITE_FORBIDDEN",
        "该路径不在本会话的写入授权范围内，请让父级授予 writeScope 后再写",
    )
}

/// 派生范围非法或试图放大父权限；子代理永远不能自行修改自己的 writeScope。
fn scope_denied() -> CommandError {
    CommandError::new(
        "AGENT_WRITE_SCOPE_DENIED",
        "写入授权范围无效或超出父级范围，请只申请父级已授权的路径",
    )
}

/// 规范、去重、排序；排序让持久化与状态输出稳定可复现。
fn canonical_list(entries: &[ScopeEntry]) -> Vec<String> {
    let mut list = entries
        .iter()
        .map(|entry| entry.canonical())
        .collect::<Vec<_>>();
    list.sort();
    list.dedup();
    list
}

/// 解析单项：拒绝空串、绝对路径、`..`、反斜杠、隐藏目录与超长路径。
fn parse_entry(raw: &str) -> Result<ScopeEntry, CommandError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ENTRY_LEN || trimmed != raw {
        return Err(scope_denied());
    }
    if let Some(prefix) = trimmed.strip_suffix('/') {
        if prefix.is_empty() {
            // 单独的 `/` 是知识库根目录前缀；子级申请它仍受父范围子集约束。
            return Ok(ScopeEntry::Dir(String::new()));
        }
        return Ok(ScopeEntry::Dir(crate::vaultfs::sanitize_agent_dir(prefix)?));
    }
    Ok(ScopeEntry::File(crate::vaultfs::sanitize_agent_note(
        trimmed,
    )?))
}

/// 只有笔记文件（`.md`）属于写入授权语义：目录授权不包含同目录下的其他文件，
/// 与 vaultfs::sanitize_agent_note 的契约保持一致。
fn is_note(path: &str) -> bool {
    path.len() > 3 && path.to_ascii_lowercase().ends_with(".md")
}

/// 路径分段；空段与多余分隔符不影响比较，别名写法不能绕过覆盖判断。
fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

/// 目录前缀覆盖判断：写入路径必须落在该目录内，且目录本身不能是文件。
fn covers_dir(entry: &str, path: &str) -> bool {
    let Some(prefix) = entry.strip_suffix('/') else {
        return false;
    };
    let parent = segments(prefix);
    parent.is_empty() || segments(path).starts_with(&parent)
}

/// 既有文件覆盖判断：目录前缀或精确文件都可以。
fn covers_path(entry: &str, path: &str) -> bool {
    if covers_dir(entry, path) {
        return true;
    }
    !entry.ends_with('/') && segments(entry) == segments(path)
}

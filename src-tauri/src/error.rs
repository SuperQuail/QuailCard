use serde::Serialize;

/// 提供给前端的统一命令错误。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    /// 创建指定错误码和消息的命令错误。
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 创建输入校验错误。
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("VALIDATION_ERROR", message)
    }

    /// 创建不暴露敏感请求内容的供应商错误。
    pub fn provider(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }
}

impl std::fmt::Display for CommandError {
    /// 将命令错误格式化为可记录的单行消息。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<std::io::Error> for CommandError {
    /// 记录完整文件系统错误，并按错误类型返回不泄露本地路径的安全消息。
    fn from(error: std::io::Error) -> Self {
        let message = match error.kind() {
            std::io::ErrorKind::NotFound => "文件或文件夹不存在",
            std::io::ErrorKind::PermissionDenied => "没有权限访问该文件",
            std::io::ErrorKind::AlreadyExists => "同名文件或文件夹已存在",
            _ => "文件操作失败，请稍后重试",
        };
        eprintln!("FILE_ERROR(detail): {error}");
        Self::new("FILE_ERROR", message)
    }
}

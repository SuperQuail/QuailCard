use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpListener,
    time::{timeout, Instant},
};
use uuid::Uuid;

use super::super::openai_oauth_helpers::{
    authorize_url, generate_random_text, parse_callback_request, read_callback_headers,
    write_callback_page, CALLBACK_ADDRESS, CALLBACK_URI,
};
use super::super::openai_oauth_token::exchange_code;
use super::{cancelled_error, BrowserLoginContext, LoginCancellation, OpenAiOAuthService};
use crate::{
    error::CommandError,
    models::{OpenAiLoginStart, ProviderSummary},
    storage::Storage,
    vault::EncryptedVault,
};

impl OpenAiOAuthService {
    /// 启动本机回调服务器和浏览器 PKCE 后台任务。
    pub(super) async fn start_browser(
        &self,
        storage: Storage,
        vault: EncryptedVault,
        provider_id: String,
    ) -> Result<OpenAiLoginStart, CommandError> {
        let listener = TcpListener::bind(CALLBACK_ADDRESS).await.map_err(|_| {
            CommandError::new(
                "OAUTH_CALLBACK_UNAVAILABLE",
                "无法监听本机 1455 端口，请关闭占用该端口的程序后重试",
            )
        })?;
        let verifier = generate_random_text(4);
        let state = generate_random_text(2);
        let challenge =
            general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let url = authorize_url(&challenge, &state)?;
        let attempt_id = Uuid::now_v7().to_string();
        let cancellation = self.insert_pending(&attempt_id, "等待浏览器授权").await;

        let service = self.clone();
        let task_attempt_id = attempt_id.clone();
        tauri::async_runtime::spawn(async move {
            let result = service
                .complete_browser_login(BrowserLoginContext {
                    storage,
                    vault,
                    provider_id,
                    listener,
                    expected_state: state,
                    verifier,
                    attempt_id: task_attempt_id.clone(),
                    cancellation,
                })
                .await;
            service.finish_attempt(&task_attempt_id, result).await;
        });
        Ok(OpenAiLoginStart {
            attempt_id,
            mode: "browser".to_string(),
            url,
            user_code: None,
        })
    }

    /// 等待本机回调、交换令牌并持久化浏览器登录结果。
    pub(super) async fn complete_browser_login(
        &self,
        context: BrowserLoginContext,
    ) -> Result<ProviderSummary, CommandError> {
        let code = wait_for_browser_code(
            context.listener,
            &context.expected_state,
            &context.cancellation,
        )
        .await?;
        self.mark_completing(&context.attempt_id).await?;
        let tokens = exchange_code(&self.client, &code, CALLBACK_URI, &context.verifier).await?;
        self.persist_credential(
            &context.storage,
            &context.vault,
            &context.provider_id,
            tokens,
        )
        .await
    }
}

/// 等待一次浏览器回调并返回经过 state 校验的授权码。
async fn wait_for_browser_code(
    listener: TcpListener,
    expected_state: &str,
    cancellation: &LoginCancellation,
) -> Result<String, CommandError> {
    let deadline = Instant::now() + Duration::from_secs(10 * 60);
    loop {
        let (mut stream, _) = tokio::select! {
            _ = cancellation.cancelled() => return Err(cancelled_error()),
            result = tokio::time::timeout_at(deadline, listener.accept()) => {
                result
                    .map_err(|_| CommandError::new("OAUTH_TIMEOUT", "浏览器登录已超时，请重新登录"))?
                    .map_err(|_| CommandError::new("OAUTH_CALLBACK_ERROR", "无法接收浏览器授权回调"))?
            }
        };
        let request = tokio::select! {
            _ = cancellation.cancelled() => return Err(cancelled_error()),
            result = timeout(Duration::from_secs(5), read_callback_headers(&mut stream)) => {
                match result {
                    Ok(Ok(request)) => request,
                    _ => continue,
                }
            }
        };
        let result = parse_callback_request(&request, expected_state);
        let terminal = result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.code == "OAUTH_AUTHORIZATION_REJECTED");
        if result.is_ok() {
            write_callback_page(
                &mut stream,
                "200 OK",
                "QuailCard 已收到授权",
                "正在安全交换登录凭据，可以关闭此页面并返回 QuailCard。",
            )
            .await;
        } else {
            write_callback_page(
                &mut stream,
                "400 Bad Request",
                "QuailCard 授权未完成",
                "请返回 QuailCard 查看详情或继续原登录流程。",
            )
            .await;
        }
        if terminal {
            return result;
        }
    }
}

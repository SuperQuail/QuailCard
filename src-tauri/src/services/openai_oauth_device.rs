use std::time::Duration;

use reqwest::StatusCode;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use super::super::openai_oauth_helpers::{
    parse_poll_interval, DeviceCodeResponse, DeviceTokenResponse, CLIENT_ID, DEVICE_REDIRECT_URI,
    ISSUER,
};
use super::super::openai_oauth_token::exchange_code;
use super::{cancelled_error, DeviceLoginContext, OpenAiOAuthService};
use crate::{
    database::Database,
    error::CommandError,
    models::{OpenAiLoginStart, ProviderSummary},
    vault::EncryptedVault,
};

impl OpenAiOAuthService {
    /// 请求设备码并启动带超时的轮询任务。
    pub(super) async fn start_device(
        &self,
        database: Database,
        vault: EncryptedVault,
        provider_id: String,
    ) -> Result<OpenAiLoginStart, CommandError> {
        let device = self.request_device_code().await?;
        let interval = parse_poll_interval(&device.interval);
        let attempt_id = Uuid::now_v7().to_string();
        let cancellation = self.insert_pending(&attempt_id, "等待输入设备码").await;

        let user_code = device.user_code.clone();
        let service = self.clone();
        let task_attempt_id = attempt_id.clone();
        tauri::async_runtime::spawn(async move {
            let result = service
                .complete_device_login(DeviceLoginContext {
                    database,
                    vault,
                    provider_id,
                    device,
                    interval,
                    attempt_id: task_attempt_id.clone(),
                    cancellation,
                })
                .await;
            service.finish_attempt(&task_attempt_id, result).await;
        });
        Ok(OpenAiLoginStart {
            attempt_id,
            mode: "device".to_string(),
            url: format!("{ISSUER}/codex/device"),
            user_code: Some(user_code),
        })
    }

    /// 轮询设备授权、交换令牌并持久化登录结果。
    pub(super) async fn complete_device_login(
        &self,
        context: DeviceLoginContext,
    ) -> Result<ProviderSummary, CommandError> {
        let deadline = Instant::now() + Duration::from_secs(15 * 60);
        loop {
            if Instant::now() >= deadline {
                return Err(CommandError::new(
                    "OAUTH_TIMEOUT",
                    "设备码登录已超时，请重新登录",
                ));
            }
            let response = tokio::select! {
                _ = context.cancellation.cancelled() => return Err(cancelled_error()),
                result = self
                    .client
                    .post(format!("{ISSUER}/api/accounts/deviceauth/token"))
                    .json(&serde_json::json!({
                        "device_auth_id": context.device.device_auth_id,
                        "user_code": context.device.user_code,
                    }))
                    .send() => result.map_err(|_| {
                        CommandError::new("OAUTH_NETWORK_ERROR", "无法查询设备码授权状态")
                    })?,
            };
            if response.status().is_success() {
                let result = response.json::<DeviceTokenResponse>().await.map_err(|_| {
                    CommandError::new("OAUTH_RESPONSE_ERROR", "设备码授权响应格式无效")
                })?;
                self.mark_completing(&context.attempt_id).await?;
                let tokens = exchange_code(
                    &self.client,
                    &result.authorization_code,
                    DEVICE_REDIRECT_URI,
                    &result.code_verifier,
                )
                .await?;
                return self
                    .persist_credential(
                        &context.database,
                        &context.vault,
                        &context.provider_id,
                        tokens,
                    )
                    .await;
            }
            if !matches!(
                response.status(),
                StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
            ) {
                return Err(CommandError::new(
                    "OAUTH_DEVICE_REJECTED",
                    format!("设备码授权失败（HTTP {}）", response.status().as_u16()),
                ));
            }
            let delay = context
                .interval
                .saturating_add(Duration::from_secs(3))
                .min(deadline.saturating_duration_since(Instant::now()));
            tokio::select! {
                _ = context.cancellation.cancelled() => return Err(cancelled_error()),
                _ = sleep(delay) => {}
            }
        }
    }

    /// 请求 OpenAI 设备登录所需的用户码。
    pub(super) async fn request_device_code(&self) -> Result<DeviceCodeResponse, CommandError> {
        let response = self
            .client
            .post(format!("{ISSUER}/api/accounts/deviceauth/usercode"))
            .json(&serde_json::json!({ "client_id": CLIENT_ID }))
            .send()
            .await
            .map_err(|_| CommandError::new("OAUTH_NETWORK_ERROR", "无法请求 OpenAI 设备码"))?;
        if !response.status().is_success() {
            return Err(CommandError::new(
                "OAUTH_DEVICE_REJECTED",
                format!(
                    "OpenAI 拒绝设备码请求（HTTP {}）",
                    response.status().as_u16()
                ),
            ));
        }
        response
            .json::<DeviceCodeResponse>()
            .await
            .map_err(|_| CommandError::new("OAUTH_RESPONSE_ERROR", "设备码响应格式无效"))
    }
}

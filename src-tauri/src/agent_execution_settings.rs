//! 自主执行只限制并发与派生资源；旧执行预算字段保留读取兼容，不再生效。
use crate::error::CommandError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentExecutionSettings {
    /// 兼容旧配置；不再限制自动续轮。
    pub max_goal_rounds: u32,
    pub max_depth: u32,
    pub max_agents: usize,
    pub max_concurrent_models: usize,
    /// 兼容旧配置；不再限制模型调用次数。
    pub max_model_calls: u32,
    /// 兼容旧配置；不再限制总执行时间。
    pub timeout_seconds: u64,
}
impl Default for AgentExecutionSettings {
    /// 只保留有界的并发与派生设置；废弃预算默认零，不参与运行判定。
    fn default() -> Self {
        Self {
            max_goal_rounds: 0,
            max_depth: 3,
            max_agents: 12,
            max_concurrent_models: 3,
            max_model_calls: 0,
            timeout_seconds: 0,
        }
    }
}
impl AgentExecutionSettings {
    /// 仅校验仍生效的资源保护，旧配置预算值不得阻止开始执行。
    pub(crate) fn validate(&self) -> Result<(), CommandError> {
        if self.max_depth > 8
            || !(1..=64).contains(&self.max_agents)
            || !(1..=16).contains(&self.max_concurrent_models)
        {
            return Err(CommandError::validation(
                "Agent 并发或派生设置无效，请检查 settings.toml 的 agent 小节",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧配置原样可读，废弃预算无论大小都不再影响执行准入。
    #[test]
    fn legacy_budgets_are_ignored() {
        for (rounds, calls, seconds) in [(16, 256, 900), (0, 0, 0), (u32::MAX, u32::MAX, u64::MAX)]
        {
            let settings: AgentExecutionSettings = serde_json::from_value(serde_json::json!({
                "maxGoalRounds": rounds, "maxModelCalls": calls, "timeoutSeconds": seconds
            }))
            .unwrap();
            settings.validate().unwrap();
            assert_eq!(settings.max_goal_rounds, rounds);
            assert_eq!(settings.max_model_calls, calls);
            assert_eq!(settings.timeout_seconds, seconds);
        }
    }

    /// settings.toml 的旧小节可直接使用，不要求用户删字段或重建配置。
    #[test]
    fn legacy_toml_settings_round_trip_without_migration() {
        let settings: AgentExecutionSettings = toml::from_str(
            "maxGoalRounds = 16\nmaxModelCalls = 256\ntimeoutSeconds = 900\nmaxConcurrentModels = 1",
        ).unwrap();
        settings.validate().unwrap();
        assert_eq!(settings.max_concurrent_models, 1);
        let restored: AgentExecutionSettings =
            toml::from_str(&toml::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        let missing: AgentExecutionSettings = toml::from_str("").unwrap();
        assert_eq!(missing, AgentExecutionSettings::default());
    }

    /// 取消执行预算不应同时取消并发与派生资源保护。
    #[test]
    fn resource_limits_remain_validated() {
        for settings in [
            AgentExecutionSettings {
                max_depth: 9,
                ..Default::default()
            },
            AgentExecutionSettings {
                max_agents: 0,
                ..Default::default()
            },
            AgentExecutionSettings {
                max_agents: 65,
                ..Default::default()
            },
            AgentExecutionSettings {
                max_concurrent_models: 0,
                ..Default::default()
            },
            AgentExecutionSettings {
                max_concurrent_models: 17,
                ..Default::default()
            },
        ] {
            assert!(settings.validate().is_err());
        }
        AgentExecutionSettings::default().validate().unwrap();
    }
}

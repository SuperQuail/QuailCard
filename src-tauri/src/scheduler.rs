use crate::models::{ReviewProgress, ReviewRating};

/// 复习调度器使用的领域状态快照。
#[derive(Debug, Clone)]
pub(crate) struct ReviewState {
    pub repetitions: i64,
    pub lapses: i64,
    pub total_reviews: i64,
    pub version: i64,
    pub stability: Option<f64>,
    pub difficulty: f64,
    pub last_review_at: Option<i64>,
    pub scheduler_phase: String,
    pub learning_step: i64,
}

/// 根据当前状态和评分计算下一次复习状态。
pub(crate) fn calculate_next_state(
    current: &ReviewState,
    rating: ReviewRating,
    now: i64,
) -> ReviewProgress {
    let difficulty = next_difficulty(current.difficulty, rating);
    let (scheduler_phase, stability, delay_seconds, repetitions, lapses) =
        match (current.scheduler_phase.as_str(), rating) {
            (_, ReviewRating::Again) => (
                if matches!(current.scheduler_phase.as_str(), "review" | "relearning") {
                    "relearning"
                } else {
                    "learning"
                },
                current.stability.map(|value| (value * 0.4).max(0.5)),
                10 * 60,
                0,
                current.lapses + 1,
            ),
            ("new" | "learning", ReviewRating::Hard) => (
                "learning",
                current.stability,
                86_400,
                current.repetitions,
                current.lapses,
            ),
            ("new" | "learning", ReviewRating::Good) if current.learning_step == 0 => (
                "learning",
                current.stability,
                86_400,
                current.repetitions,
                current.lapses,
            ),
            ("new" | "learning", ReviewRating::Good) => {
                let initial = initial_stability(difficulty);
                (
                    "review",
                    Some(initial),
                    days_to_seconds(initial),
                    1,
                    current.lapses,
                )
            }
            ("relearning", ReviewRating::Hard) => (
                "relearning",
                current.stability,
                86_400,
                current.repetitions,
                current.lapses,
            ),
            ("relearning", ReviewRating::Good) => {
                let recovered = current.stability.unwrap_or(1.0).max(1.0);
                (
                    "review",
                    Some(recovered),
                    days_to_seconds(recovered),
                    1,
                    current.lapses,
                )
            }
            (_, ReviewRating::Hard) => {
                let next = grow_stability(current, difficulty, false, now);
                (
                    "review",
                    Some(next),
                    days_to_seconds(next),
                    current.repetitions + 1,
                    current.lapses,
                )
            }
            (_, ReviewRating::Good) => {
                let next = grow_stability(current, difficulty, true, now);
                (
                    "review",
                    Some(next),
                    days_to_seconds(next),
                    current.repetitions + 1,
                    current.lapses,
                )
            }
        };
    let interval_days = (delay_seconds / 86_400).clamp(0, 365);
    ReviewProgress {
        due_at: now + delay_seconds,
        interval_days,
        repetitions,
        lapses,
        total_reviews: current.total_reviews + 1,
        version: current.version + 1,
        scheduler_phase: scheduler_phase.to_string(),
        stability,
        difficulty,
    }
}

/// 计算新卡和重学卡下一次使用的学习步骤。
pub(crate) fn next_learning_step(current: &ReviewState, rating: ReviewRating) -> i64 {
    match rating {
        ReviewRating::Again => 0,
        ReviewRating::Hard => current.learning_step.max(1),
        ReviewRating::Good if current.scheduler_phase == "new" => 1,
        ReviewRating::Good if current.scheduler_phase == "learning" => {
            (current.learning_step + 1).min(2)
        }
        ReviewRating::Good => 0,
    }
}

/// 根据本次评分更新卡片难度，范围固定为 1-10。
fn next_difficulty(current: f64, rating: ReviewRating) -> f64 {
    let delta = match rating {
        ReviewRating::Again => 1.0,
        ReviewRating::Hard => 0.35,
        ReviewRating::Good => -0.25,
    };
    (current + delta).clamp(1.0, 10.0)
}

/// 为完成两次跨会话提取的新卡计算初始稳定性。
fn initial_stability(difficulty: f64) -> f64 {
    (4.5 - difficulty * 0.35).clamp(1.0, 4.0)
}

/// 根据实际经过时间、预测回忆率和评分增长稳定性。
fn grow_stability(current: &ReviewState, difficulty: f64, good: bool, now: i64) -> f64 {
    let stability = current.stability.unwrap_or(1.0).clamp(0.5, 365.0);
    let elapsed_days = current
        .last_review_at
        .map(|last| ((now - last).max(0) as f64 / 86_400.0).max(0.01))
        .unwrap_or(stability);
    let retrievability = 0.9_f64.powf(elapsed_days / stability);
    let desirable_difficulty = (1.0 - retrievability).clamp(0.0, 0.8);
    let factor = if good {
        1.55 + desirable_difficulty * 2.0 + (10.0 - difficulty) * 0.025
    } else {
        1.1 + desirable_difficulty * 0.5
    };
    (stability * factor).clamp(1.0, 365.0)
}

/// 将长期稳定性天数转换为到期秒数。
fn days_to_seconds(days: f64) -> i64 {
    (days.clamp(1.0, 365.0) * 86_400.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建调度测试使用的初始状态。
    fn initial_state() -> ReviewState {
        ReviewState {
            repetitions: 0,
            lapses: 0,
            total_reviews: 0,
            version: 0,
            stability: None,
            difficulty: 5.0,
            last_review_at: None,
            scheduler_phase: "new".to_string(),
            learning_step: 0,
        }
    }

    #[test]
    /// 新卡首次记得后进入跨会话学习步骤。
    fn good_new_card_uses_learning_step() {
        let next = calculate_next_state(&initial_state(), ReviewRating::Good, 100);
        assert_eq!(next.interval_days, 1);
        assert_eq!(next.scheduler_phase, "learning");
        assert_eq!(next.due_at, 100 + 86_400);
    }

    #[test]
    /// 新卡完成第二次跨会话提取后进入长期复习。
    fn second_good_graduates_learning_card() {
        let current = ReviewState {
            scheduler_phase: "learning".to_string(),
            learning_step: 1,
            ..initial_state()
        };
        let next = calculate_next_state(&current, ReviewRating::Good, 100);
        assert_eq!(next.scheduler_phase, "review");
        assert_eq!(next.repetitions, 1);
        assert!(next.stability.is_some());
    }

    #[test]
    /// 延迟后仍能回忆会比按时回忆获得更高的稳定性增长。
    fn delayed_recall_rewards_desirable_difficulty() {
        let current = ReviewState {
            repetitions: 2,
            stability: Some(10.0),
            last_review_at: Some(100),
            scheduler_phase: "review".to_string(),
            ..initial_state()
        };
        let on_time = calculate_next_state(&current, ReviewRating::Good, 100 + 10 * 86_400);
        let delayed = calculate_next_state(&current, ReviewRating::Good, 100 + 20 * 86_400);
        assert!(delayed.stability > on_time.stability);
    }

    #[test]
    /// 忘记会增加遗忘次数并重置重复次数。
    fn again_resets_repetitions() {
        let current = ReviewState {
            repetitions: 5,
            stability: Some(20.0),
            scheduler_phase: "review".to_string(),
            ..initial_state()
        };
        let next = calculate_next_state(&current, ReviewRating::Again, 100);
        assert_eq!(next.repetitions, 0);
        assert_eq!(next.lapses, 1);
        assert_eq!(next.interval_days, 0);
        assert_eq!(next.scheduler_phase, "relearning");
        assert_eq!(next.due_at, 100 + 10 * 60);
    }
}

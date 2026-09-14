//! 终态持久化及生成期间取消的回归测试。
use super::*;
use crate::ai::ToolDefinition;
use crate::services::{
    agent_ports::{AgentFuture, AgentModelReply},
    video_tasks::VideoTaskRegistry,
};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// 未连接网络的任务输入，测试只使用存储与模型端口。
fn input() -> PipelineInput {
    let crate::video::url::VideoInput::Video(video) = crate::video::url::parse("av42").unwrap()
    else {
        panic!("视频输入")
    };
    PipelineInput {
        video,
        pages: vec![],
        quality: None,
        screenshots: false,
        note: true,
        mode: crate::video::models::VideoOutputMode::Note,
        force_transcribe: false,
    }
}

/// 取消与失败都写入任务记录，不删除既有转录或用户笔记。
#[test]
fn failed_and_cancelled_records_are_persisted() {
    let root = std::env::temp_dir().join(format!("qc-lifecycle-{}", uuid::Uuid::now_v7()));
    // 存储层要求已存在的知识库根目录；夹具自行创建，不能依赖写操作隐式建库。
    std::fs::create_dir_all(&root).unwrap();
    let storage = VideoStorage::new(&root);
    let input = input();
    for code in ["VIDEO_CANCELLED", "VIDEO_NOTE_FAILED"] {
        let record = VideoTaskRecord::new(code, &input.video.cache_key, &input.video.source_url);
        storage.save_task(&record).unwrap();
        let control = VideoTaskRegistry::default()
            .register("owner", &record)
            .unwrap();
        finish_record(
            &storage,
            &input,
            &control,
            &Err(CommandError::new(code, "测试错误")),
        )
        .unwrap();
        let saved = storage.load_task(code).unwrap().unwrap();
        assert_eq!(
            saved.state,
            if code == "VIDEO_CANCELLED" {
                "cancelled"
            } else {
                "failed"
            }
        );
        assert_eq!(saved.error.as_deref(), Some("测试错误"));
    }
    std::fs::remove_dir_all(root).unwrap();
}

struct PendingModel {
    started: Arc<AtomicBool>,
}
impl AgentModel for PendingModel {
    /// 永不完成的请求让测试确认取消不依赖网络超时。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            self.started.store(true, Ordering::SeqCst);
            std::future::pending().await
        })
    }
}

/// LLM 已开始仍可取消，生成结果无法越过写入前的错误传播。
#[tokio::test]
async fn cancel_during_model_generation() {
    let started = Arc::new(AtomicBool::new(false));
    let model = PendingModel {
        started: started.clone(),
    };
    let record = VideoTaskRecord::new("cancel-generation", "key", "url");
    let control = VideoTaskRegistry::default()
        .register("owner", &record)
        .unwrap();
    let transcript = Transcript {
        language: "zh".into(),
        source: "whisper".into(),
        segments: vec![],
    };
    let meta = NoteMeta {
        title: "test",
        owner: "test",
        duration: 1.0,
        source_url: "url",
        model_label: "test",
        transcript_source: "whisper",
        max_shots: 0,
    };
    let cancel = async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        control.cancel();
    };
    let generate = generate_note(&model, &transcript, &meta, &|_| {}, &control);
    let (result, _) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(generate, cancel)
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().code, "VIDEO_CANCELLED");
}

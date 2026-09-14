//! 视频转笔记命令：只做参数校验与委派，不实现业务规则。

use tauri::{Manager, Window};

use crate::{
    error::CommandError,
    paths,
    services::AppServices,
    storage::Storage,
    video::{
        asr,
        bilibili::media,
        components::{self, Component},
        models::{
            VideoComponentStatus, VideoDownloadStatus, VideoModelStatus, VideoPage, VideoProbe,
            VideoQuality, VideoSettings, VideoStartInput, VideoTaskHistory, VideoTaskStatus,
        },
    },
    video_bridge,
};

/// 解析链接并返回视频信息与可用清晰度，供界面先展示再开始。
#[tauri::command]
pub async fn video_probe(app: tauri::AppHandle, url: String) -> Result<VideoProbe, CommandError> {
    let client = video_bridge::client(&app).await?;
    let video = video_bridge::resolve_video(&client, &url).await?;
    let info = media::video_info(&client, &video).await?;
    let page = video
        .page
        .and_then(|page| info.pages.iter().find(|item| item.page == page))
        .cloned()
        .unwrap_or_else(|| info.pages[0].clone());
    let keys = media::wbi_keys(&client).await?;
    let play = media::playurl(&client, &keys, info.aid, page.cid, page.duration).await?;
    Ok(VideoProbe {
        bvid: info.bvid.clone(),
        title: info.title.clone(),
        owner: info.owner.clone(),
        duration: info.duration,
        pages: info
            .pages
            .iter()
            .map(|page| VideoPage {
                page: page.page,
                title: page.title.clone(),
                duration: page.duration,
            })
            .collect(),
        qualities: play
            .qualities
            .into_iter()
            .map(|quality| VideoQuality {
                qn: quality.qn,
                label: quality.label,
                height: quality.height,
                available: quality.available,
                requires_vip: quality.requires_vip,
                estimated_bytes: quality.estimated_bytes,
                estimated_duration: quality.estimated_duration,
                unavailable_reason: quality.unavailable_reason,
            })
            .collect(),
        selected_page: page.page,
        logged_in: client.cookie().is_logged_in(),
    })
}

/// 开始视频任务：登记后立即返回，后台执行。
#[tauri::command]
pub async fn video_task_start(
    app: tauri::AppHandle,
    window: Window,
    input: VideoStartInput,
) -> Result<VideoTaskStatus, CommandError> {
    video_bridge::start(&app, window.label(), input).await
}

/// 查询任务快照。
#[tauri::command]
pub fn video_task_status(
    app: tauri::AppHandle,
    window: Window,
    id: String,
) -> Result<VideoTaskStatus, CommandError> {
    app.state::<AppServices>()
        .video_tasks
        .status(window.label(), &id)
}

/// 停止任务；已完成的操作保留。
#[tauri::command]
pub fn video_task_cancel(
    app: tauri::AppHandle,
    window: Window,
    id: String,
) -> Result<(), CommandError> {
    app.state::<AppServices>()
        .video_tasks
        .cancel(window.label(), &id)
}

/// 组件状态列表。
#[tauri::command]
pub async fn video_components(
    app: tauri::AppHandle,
) -> Result<Vec<VideoComponentStatus>, CommandError> {
    let settings = app.state::<Storage>().get_video_settings().await;
    let entries = [
        (Component::Ffmpeg, "ffmpeg", "ffmpeg（媒体解码与截图）"),
        (Component::Whisper, "whisper", "whisper-cli（本地转写）"),
    ];
    let mut result = Vec::new();
    for (component, id, label) in entries {
        let path = video_bridge::component_path(&app, &settings, component);
        let mut entry = VideoComponentStatus {
            id: id.to_string(),
            name: label.to_string(),
            available: path.is_file(),
            path: paths::simplified(&path),
            source: if path.is_file() {
                "已定位"
            } else {
                "未找到"
            }
            .into(),
            detail: String::new(),
        };
        if component == Component::Whisper && entry.available {
            let probe = components::probe_whisper(&path).await;
            entry.available = probe.runnable;
            entry.source = if probe.gpu_available {
                probe.gpu_backend.unwrap_or_else(|| "GPU".into())
            } else if probe.runnable {
                "可运行（设备以任务诊断为准）".into()
            } else {
                "自检失败".into()
            };
            entry.detail = probe.detail;
            if !entry.available {
                if let Some(cpu) = components::cpu_fallback(&path) {
                    let fallback = components::probe_whisper(&cpu).await;
                    if fallback.runnable {
                        entry.available = true;
                        entry.source = "CPU 回退可用".into();
                        entry.detail.push_str("；独立 CPU 组件自检通过");
                    }
                }
            }
        }
        result.push(entry);
    }
    Ok(result)
}

/// 语音模型清单与本地状态。
#[tauri::command]
pub async fn video_models(app: tauri::AppHandle) -> Result<Vec<VideoModelStatus>, CommandError> {
    let data = video_bridge::data_dir(&app)?;
    Ok(asr::MODELS
        .iter()
        .map(|option| VideoModelStatus {
            id: option.id.to_string(),
            label: option.label.to_string(),
            bytes: option.bytes,
            status: asr::status(&data, option).to_string(),
        })
        .collect())
}

/// 下载语音模型；可在下载过程中取消。
#[tauri::command]
pub async fn video_download_model(
    app: tauri::AppHandle,
    window: Window,
    id: String,
) -> Result<VideoModelStatus, CommandError> {
    let option = asr::find(&id).ok_or_else(|| CommandError::validation("未知的模型档位"))?;
    let services = app.state::<AppServices>();
    let settings = app.state::<Storage>().get_video_settings().await;
    let data = video_bridge::data_dir(&app)?;
    let handle = services.video_downloads.begin(window.label(), &id)?;
    handle.progress(0, Some(option.bytes));
    let cancel = || handle.is_cancelled();
    let mut progress = |written, total| handle.progress(written, total);
    let result = asr::download(
        option,
        &data,
        Some(&settings.model_mirror),
        &cancel,
        &mut progress,
    )
    .await;
    if result.is_ok() {
        handle.progress(option.bytes, Some(option.bytes));
    }
    handle.finish(result.as_ref().err());
    result?;
    Ok(VideoModelStatus {
        id: option.id.to_string(),
        label: option.label.to_string(),
        bytes: option.bytes,
        status: asr::status(&data, option).to_string(),
    })
}

/// 取消模型下载。
#[tauri::command]
pub fn video_model_cancel(app: tauri::AppHandle, window: Window) -> Result<(), CommandError> {
    app.state::<AppServices>()
        .video_downloads
        .cancel(window.label());
    Ok(())
}

/// 读取视频设置。
#[tauri::command]
pub async fn video_get_settings(app: tauri::AppHandle) -> Result<VideoSettings, CommandError> {
    Ok(app.state::<Storage>().get_video_settings().await)
}

/// 保存视频设置并回读，保证前端看到落盘结果。
#[tauri::command]
pub async fn video_save_settings(
    app: tauri::AppHandle,
    settings: VideoSettings,
) -> Result<VideoSettings, CommandError> {
    let storage = app.state::<Storage>();
    storage.set_video_settings(&settings).await?;
    Ok(storage.get_video_settings().await)
}

/// 查询模型下载进度；不会启动或重启下载。
#[tauri::command]
pub fn video_download_status(
    app: tauri::AppHandle,
    window: Window,
) -> Result<VideoDownloadStatus, CommandError> {
    Ok(app
        .state::<AppServices>()
        .video_downloads
        .status(window.label()))
}

/// 查询历史任务；只返回手动发起的任务，旧进程的运行中任务标记为已中断，不会自动执行。
#[tauri::command]
pub async fn video_history(
    app: tauri::AppHandle,
    window: Window,
) -> Result<Vec<VideoTaskHistory>, CommandError> {
    crate::video_history::for_user(&app, window.label())
}

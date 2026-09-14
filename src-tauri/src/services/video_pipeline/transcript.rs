//! 转录收集与组件准备。
use super::*;
#[path = "asr_progress.rs"]
mod asr_progress;

/// 取字：模型身份分 P 缓存 → 平台字幕 → 本地转写；强制转写跳过前两步。
///
/// 分 P 缓存以 (视频号, 模型, cid) 为键，换模型不能复用旧的本地转写。
pub(super) async fn collect_transcript(
    deps: &PipelineDeps<'_>,
    control: &VideoControl,
    info: &media::VideoInfo,
    input: &PipelineInput,
    cancel: &CancelHandle,
) -> Result<Transcript, CommandError> {
    let storage = VideoStorage::new(&deps.vault_root);
    let task_id = control.snapshot().task_id;
    let key = cache_key(&input.video, info.aid, &deps.settings);
    let selected = selected_pages(info, input);
    let total: f64 = selected
        .iter()
        .map(|page| page.duration)
        .sum::<f64>()
        .max(1.0);
    let mut done = 0.0;
    let mut pages: Vec<(u32, String, f64, Vec<Segment>)> = Vec::new();
    let mut missing: Vec<media::PageInfo> = Vec::new();
    let mut source = String::new();
    for page in &selected {
        if cancel() {
            return Err(cancelled());
        }
        let cached = if input.force_transcribe {
            None
        } else {
            storage
                .load_page_transcript(&key, page.cid)?
                .filter(|cached| {
                    cache_source_valid(&cached.source, &deps.settings)
                        && !transcript::quality(cached, page.duration).insufficient
                })
        };
        if let Some(cached) = cached {
            if source.is_empty() {
                source = cached.source.clone();
            }
            pages.push((
                page.page,
                page.title.clone(),
                page.duration,
                cached.segments,
            ));
            done += page.duration;
            continue;
        }
        match if input.force_transcribe {
            None
        } else {
            media::subtitle(&deps.client, &input.video, page.cid).await?
        } {
            Some((page_source, payload)) => {
                if cancel() {
                    return Err(cancelled());
                }
                let segments = transcript::parse_subtitle_json(&payload)?;
                storage.save_page_transcript(
                    &key,
                    page.cid,
                    &Transcript {
                        language: "zh".to_string(),
                        source: page_source.clone(),
                        segments: segments.clone(),
                    },
                )?;
                if source.is_empty() {
                    source = page_source;
                }
                pages.push((page.page, page.title.clone(), page.duration, segments));
                done += page.duration;
            }
            None => missing.push(page.clone()),
        }
    }
    if !missing.is_empty() {
        if !deps.settings.asr_enabled {
            return Err(CommandError::new(
                "VIDEO_NO_SUBTITLE",
                "该视频没有可用字幕；可在设置中启用本地转写后重试",
            ));
        }
        let (model, ffmpeg, whisper) = prepare_components(deps)?;
        let keys = media::wbi_keys(&deps.client).await?;
        let referer = input.video.page_url();
        let count = missing.len();
        for (index, page) in missing.iter().enumerate() {
            if cancel() {
                return Err(cancelled());
            }
            // 进度按分 P 时长加权，多 P 时不会来回跳。
            let base = 25.0 + 40.0 * (done / total);
            let span = 40.0 * (page.duration / total);
            control.update(|status| {
                status.step = format!("下载音频 P{}（{}/{}）", page.page, index + 1, count);
                status.asr_progress = None;
                status.progress = base as u8;
            });
            let play =
                media::playurl(&deps.client, &keys, info.aid, page.cid, page.duration).await?;
            let audio_path = storage.media_file(&task_id, &format!("audio_p{}.m4s", page.page))?;
            download::download_first(
                &deps.client,
                &play.tracks.audio,
                &referer,
                &audio_path,
                &**cancel,
                &mut |_, _| {},
            )
            .await?;
            let wav_path = storage.media_file(&task_id, &format!("audio_p{}.wav", page.page))?;
            control.update(|status| status.step = format!("转换音频 P{}", page.page));
            deps.tools
                .audio
                .export_wav(&audio_path, &wav_path, cancel.clone())
                .await?;
            let options = AsrOptions {
                model: model.clone(),
                language: Some("zh".to_string()),
                initial_prompt: Some(info.title.clone()),
                disable_gpu: false,
            };
            let progress_handle =
                asr_progress::observe(control, page.page, page.duration, 1, base, span);
            let backend_control = control.clone();
            let backend: crate::video::media::BackendHandle = std::sync::Arc::new(move |name| {
                if ["cpu", "vulkan", "cuda", "metal"].contains(&name) {
                    backend_control.update(|status| {
                        status.backend = name.to_string();
                        status.message = format!("实际转写后端：{name}");
                    });
                }
            });
            let transcript = match deps
                .tools
                .asr
                .transcribe_observed(
                    &wav_path,
                    &options,
                    cancel.clone(),
                    progress_handle.clone(),
                    backend.clone(),
                )
                .await
            {
                Ok(transcript) => transcript,
                Err(error) if error.code == "VIDEO_ASR_GPU_FAILED" => {
                    let fallback = AsrOptions {
                        disable_gpu: true,
                        ..options
                    };
                    deps.tools
                        .asr
                        .transcribe_observed(
                            &wav_path,
                            &fallback,
                            cancel.clone(),
                            asr_progress::observe(control, page.page, page.duration, 2, base, span),
                            backend,
                        )
                        .await?
                }
                Err(error) => return Err(error),
            };
            if cancel() {
                return Err(cancelled());
            }
            control.update(|status| status.asr_progress = None);
            storage.save_page_transcript(
                &key,
                page.cid,
                &Transcript {
                    language: "zh".to_string(),
                    source: "whisper".to_string(),
                    segments: transcript.segments.clone(),
                },
            )?;
            source = "whisper".to_string();
            pages.push((
                page.page,
                page.title.clone(),
                page.duration,
                transcript.segments,
            ));
            done += page.duration;
            // 仅由存储层删除已验证的托管媒体，逐 P 回收 WAV 避免磁盘峰值累积。
            storage.cleanup_task_media(&task_id)?;
        }
        let _ = (ffmpeg, whisper);
    }
    if pages.is_empty() {
        return Err(CommandError::new(
            "VIDEO_NO_TRANSCRIPT",
            "没有取得任何转录内容",
        ));
    }
    pages.sort_by_key(|(page, _, _, _)| *page);
    Ok(Transcript {
        language: "zh".to_string(),
        source,
        segments: transcript::merge_pages(&pages),
    })
}

/// 缓存版本与模型选择进入键；旧的无身份缓存只读保留但不再复用。
fn cache_key(video: &VideoRef, aid: u64, settings: &VideoSettings) -> String {
    format!("{}-v2-model-{}", video_key(video, aid), settings.asr_model)
}

/// 仅接受已知来源，关闭 ASR 后不得悄悄复用本地转写。
fn cache_source_valid(source: &str, settings: &VideoSettings) -> bool {
    ["bilibili_ai", "bilibili_cc"].contains(&source)
        || (source == "whisper" && settings.asr_enabled)
}

/// 解析并校验外部组件与模型，缺失时给出可操作的提示。
fn prepare_components(
    deps: &PipelineDeps<'_>,
) -> Result<(PathBuf, PathBuf, PathBuf), CommandError> {
    let ffmpeg = resolve_component(
        deps,
        Component::Ffmpeg,
        &deps.settings.ffmpeg_path,
        "媒体组件（ffmpeg）",
    )?;
    let whisper = resolve_component(
        deps,
        Component::Whisper,
        &deps.settings.whisper_path,
        "语音组件（whisper-cli）",
    )?;
    let option = asr::find(&deps.settings.asr_model).unwrap_or(&asr::MODELS[1]);
    if asr::status(&deps.data_dir, option) != "ready" {
        return Err(CommandError::new(
            "VIDEO_ASR_MODEL_MISSING",
            "语音模型尚未下载，请先在设置中下载模型",
        ));
    }
    Ok((asr::model_file(&deps.data_dir, option), ffmpeg, whisper))
}

/// 组件解析；所有来源都找不到时返回带指引的错误。
pub(super) fn resolve_component(
    deps: &PipelineDeps<'_>,
    component: Component,
    override_path: &str,
    label: &str,
) -> Result<PathBuf, CommandError> {
    components::resolve(
        component,
        Some(override_path),
        deps.resource_dir.as_deref(),
        Some(&deps.data_dir),
        Some(&deps.manifest_dir),
    )
    .map(|resolved| resolved.path)
    .ok_or_else(|| {
        CommandError::new(
            "VIDEO_COMPONENT_MISSING",
            format!("缺少{label}，请重新安装或指定其路径"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 模型切换必须改变缓存身份，关闭本地转写也不能复用 whisper 来源。
    #[test]
    fn cache_identity_tracks_model_and_source_policy() {
        let crate::video::url::VideoInput::Video(video) = crate::video::url::parse("av42").unwrap()
        else {
            panic!("视频输入")
        };
        let mut settings = VideoSettings::default();
        let small = cache_key(&video, 42, &settings);
        settings.asr_model = "medium".into();
        assert_ne!(small, cache_key(&video, 42, &settings));
        assert!(cache_source_valid("whisper", &settings));
        settings.asr_enabled = false;
        assert!(!cache_source_valid("whisper", &settings));
        assert!(cache_source_valid("bilibili_cc", &settings));
        assert!(!cache_source_valid("bilibili_unknown", &settings));
    }
}

//! 手动联网验收：复现主 CDN 被拒后自动回退；默认不联网，不打印签名或凭据。
use super::*;
use crate::video::{
    bilibili::{
        http::{BiliClient, CookieJar},
        media as bili,
    },
    url::{self, VideoInput},
};

/// 仅删除本测试创建的临时目录，不接触知识库或用户笔记。
struct Scratch(PathBuf);
impl Drop for Scratch {
    /// 退出测试时仅回收本次创建的临时目录。
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 显式指定 QC_FRAME_SMOKE_URL 与 QC_FRAME_SMOKE_FFMPEG 后才能验证真实媒体链路。
#[tokio::test]
#[ignore = "requires explicit video URL, FFmpeg path and network"]
async fn real_video_backup_frames() {
    let input = std::env::var("QC_FRAME_SMOKE_URL").expect("set QC_FRAME_SMOKE_URL");
    let program = std::env::var("QC_FRAME_SMOKE_FFMPEG").expect("set QC_FRAME_SMOKE_FFMPEG");
    let VideoInput::Video(video) = url::parse(&input).unwrap() else {
        panic!("direct video URL required")
    };
    let client = BiliClient::new(CookieJar::default()).unwrap();
    let info = bili::video_info(&client, &video).await.unwrap();
    let page = info
        .pages
        .iter()
        .find(|p| p.page == video.page.unwrap_or(1))
        .unwrap();
    let keys = bili::wbi_keys(&client).await.unwrap();
    let play = bili::playurl_quality(&client, &keys, info.aid, page.cid, page.duration, None)
        .await
        .unwrap();
    let track = bili::choose_video_preferred(&play.tracks.videos, None, Some(7)).unwrap();
    // 明确不安全的首地址必须被原策略拒绝，不能放宽白名单来让测试通过。
    let mut urls = vec!["https://127.0.0.1:8443/not-media".to_string(), track.url];
    urls.extend(track.backup);
    let scratch =
        Scratch(std::env::temp_dir().join(format!("qc-frame-smoke-{}", uuid::Uuid::now_v7())));
    std::fs::create_dir(&scratch.0).unwrap();
    let ffmpeg = ffmpeg::Ffmpeg::new(program.into());
    for seconds in [5.0, 60.0, 500.0, 1352.0, 2500.0, 4000.0] {
        assert!(seconds < page.duration);
        let output = scratch.0.join(format!("{seconds}.jpg"));
        tokio::time::timeout(
            std::time::Duration::from_secs(370),
            ffmpeg.extract(
                FrameSource::RemoteCandidates { urls: urls.clone() },
                seconds,
                &output,
                1600,
                Arc::new(|| false),
            ),
        )
        .await
        .expect("frame extraction exceeded bounded candidates")
        .unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.len() > 100);
        assert!(bytes.starts_with(&[0xff, 0xd8]));
        println!("FRAME_OK seconds={seconds} jpeg_bytes={}", bytes.len());
    }
}

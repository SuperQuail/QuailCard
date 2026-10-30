use std::io::Cursor;

use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

use super::{check_pixel_budget, decode_rgba};

/// 生成一张 2x2 的已知 PNG：四个像素颜色不同，便于验证解码没有串行。
fn sample_png() -> Vec<u8> {
    let mut image = RgbaImage::new(2, 2);
    image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    image.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
    image.put_pixel(0, 1, Rgba([0, 0, 255, 255]));
    image.put_pixel(1, 1, Rgba([9, 9, 9, 128]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
/// 有效 PNG 必须解出正确的宽高与 RGBA 像素顺序。
fn decodes_png_to_rgba() {
    let decoded = decode_rgba(&sample_png()).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 2));
    assert_eq!(decoded.rgba.len(), 2 * 2 * 4);
    assert_eq!(&decoded.rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&decoded.rgba[8..12], &[0, 0, 255, 255]);
}

#[test]
/// 有效 JPEG 也要能解码：附件导入允许 JPEG，复制不能只支持 PNG。
fn decodes_jpeg_to_rgba() {
    // JPEG 编码器不支持 RGBA，测试用 RGB 生成样本，解码后仍应得到 RGBA。
    let mut image = RgbImage::new(4, 4);
    image.put_pixel(0, 0, Rgb([10, 20, 30]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
        .unwrap();
    let decoded = decode_rgba(&bytes).unwrap();
    assert_eq!((decoded.width, decoded.height), (4, 4));
    assert_eq!(decoded.rgba.len(), 4 * 4 * 4);
}

#[test]
/// 非图片字节必须报固定的解码错误码，而不是文件系统或解码器原文。
fn rejects_non_image_bytes() {
    let error = decode_rgba(b"not an image at all").unwrap_err();
    assert_eq!(error.code, "IMAGE_DECODE_ERROR");
}

#[test]
/// 截断的 PNG 同样按解码失败处理，不让半张图进入剪贴板。
fn rejects_truncated_png() {
    let mut bytes = sample_png();
    bytes.truncate(bytes.len() / 2);
    let error = decode_rgba(&bytes).unwrap_err();
    assert_eq!(error.code, "IMAGE_DECODE_ERROR");
}

#[test]
/// 像素总量超限时拒绝：单边合规但总面积过大同样要拦下。
fn rejects_oversized_pixel_budget() {
    assert!(check_pixel_budget(10_000, 4_000).is_ok());
    let error = check_pixel_budget(10_000, 10_000).unwrap_err();
    assert_eq!(error.code, "VALIDATION_ERROR");
}

#[test]
/// 零尺寸图片没有可复制的内容，统一按解码失败处理。
fn rejects_zero_sized_image() {
    assert_eq!(
        check_pixel_budget(0, 10).unwrap_err().code,
        "IMAGE_DECODE_ERROR"
    );
}

#[test]
#[ignore]
/// 真实剪贴板冒烟测试：手动执行并粘贴到其他程序验证位图存在。
fn copies_real_image_to_clipboard() {
    let image = decode_rgba(&sample_png()).unwrap();
    super::copy_to_clipboard(image).unwrap();
}

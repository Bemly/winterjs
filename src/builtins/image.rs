//! WinterJS.image 底座：位图编解码（`image` 0.25）+ SVG 光栅（`resvg`）+
//! JXL 解码（`jxl-oxide`）。解码一律 RGBA8（`to_rgba8`，16 位截断记档）；
//! 动画只取首帧（gif/webp 首帧；jxl animations 上游未实现，记档）。
//! 约定：元信息走 JSON 桥，像素走 Uint8Array（`crypto/common.rs` 同款桥）。
//! dds 双 false（image 0.25 不支持，显式拒绝）；avif 不在树内（见 Cargo 注释）。

use std::io::Cursor;

use image::codecs::gif::Repeat;
use image::{ExtendedColorType, ImageEncoder, ImageFormat};
use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use crate::builtins::crypto::common::set_rval_bytes;

/// 单边上限（防 OOM；超限报 `RangeError`，错误/边界用例覆盖）。
const MAX_DIM: u32 = 16384;
/// 像素上限（w*h；RGBA8 下 64M 像素 = 256MB）。
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ImgFmt {
    Png,
    Jpeg,
    Gif,
    WebP,
    Tiff,
    Tga,
    Bmp,
    Ico,
    Hdr,
    Exr,
    Pnm,
    Farbfeld,
    Qoi,
    Svg,
    Jxl,
}

/// 格式名归一（小写 + 别名；`None` = 不支持，调用方按 encode/decode 分报错）。
fn parse_format(s: &str) -> Option<ImgFmt> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "png" => ImgFmt::Png,
        "jpg" | "jpeg" => ImgFmt::Jpeg,
        "gif" => ImgFmt::Gif,
        "webp" => ImgFmt::WebP,
        "tiff" | "tif" => ImgFmt::Tiff,
        "tga" => ImgFmt::Tga,
        "bmp" => ImgFmt::Bmp,
        "ico" => ImgFmt::Ico,
        "hdr" => ImgFmt::Hdr,
        "exr" => ImgFmt::Exr,
        "pnm" | "ppm" | "pgm" | "pbm" | "pam" => ImgFmt::Pnm,
        "farbfeld" | "ff" => ImgFmt::Farbfeld,
        "qoi" => ImgFmt::Qoi,
        "svg" | "svgz" => ImgFmt::Svg,
        "jxl" => ImgFmt::Jxl,
        _ => return None,
    })
}

fn fmt_name(f: ImgFmt) -> &'static str {
    match f {
        ImgFmt::Png => "png",
        ImgFmt::Jpeg => "jpeg",
        ImgFmt::Gif => "gif",
        ImgFmt::WebP => "webp",
        ImgFmt::Tiff => "tiff",
        ImgFmt::Tga => "tga",
        ImgFmt::Bmp => "bmp",
        ImgFmt::Ico => "ico",
        ImgFmt::Hdr => "hdr",
        ImgFmt::Exr => "exr",
        ImgFmt::Pnm => "pnm",
        ImgFmt::Farbfeld => "farbfeld",
        ImgFmt::Qoi => "qoi",
        ImgFmt::Svg => "svg",
        ImgFmt::Jxl => "jxl",
    }
}

fn fmt_mime(f: ImgFmt) -> &'static str {
    match f {
        ImgFmt::Png => "image/png",
        ImgFmt::Jpeg => "image/jpeg",
        ImgFmt::Gif => "image/gif",
        ImgFmt::WebP => "image/webp",
        ImgFmt::Tiff => "image/tiff",
        ImgFmt::Tga => "image/x-targa",
        ImgFmt::Bmp => "image/bmp",
        ImgFmt::Ico => "image/x-icon",
        ImgFmt::Hdr => "image/vnd.radiance",
        ImgFmt::Exr => "image/x-exr",
        ImgFmt::Pnm => "image/x-portable-anymap",
        ImgFmt::Farbfeld => "application/octet-stream",
        ImgFmt::Qoi => "image/x-qoi",
        ImgFmt::Svg => "image/svg+xml",
        ImgFmt::Jxl => "image/jxl",
    }
}

/// 魔数嗅探（raster 走 `image::guess_format`；svg/jxl 另判；dds 显式拒绝）。
fn sniff(bytes: &[u8]) -> Option<ImgFmt> {
    let t = bytes
        .iter()
        .take(512)
        .skip_while(|b| b.is_ascii_whitespace())
        .take(8)
        .copied()
        .collect::<Vec<u8>>();
    if t.starts_with(b"<svg") || t.starts_with(b"<?xml") {
        return Some(ImgFmt::Svg);
    }
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        return Some(ImgFmt::Svg);
    }
    if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] == 0x0a {
        return Some(ImgFmt::Jxl);
    }
    if bytes.len() >= 12 && &bytes[4..12] == b"JXL \x0d\x0a\x87\x0a" {
        return Some(ImgFmt::Jxl);
    }
    match image::guess_format(bytes) {
        Ok(ImageFormat::Dds) => None,
        Ok(f) => match f {
            ImageFormat::Png => Some(ImgFmt::Png),
            ImageFormat::Jpeg => Some(ImgFmt::Jpeg),
            ImageFormat::Gif => Some(ImgFmt::Gif),
            ImageFormat::WebP => Some(ImgFmt::WebP),
            ImageFormat::Tiff => Some(ImgFmt::Tiff),
            ImageFormat::Tga => Some(ImgFmt::Tga),
            ImageFormat::Bmp => Some(ImgFmt::Bmp),
            ImageFormat::Ico => Some(ImgFmt::Ico),
            ImageFormat::Hdr => Some(ImgFmt::Hdr),
            ImageFormat::OpenExr => Some(ImgFmt::Exr),
            ImageFormat::Pnm => Some(ImgFmt::Pnm),
            ImageFormat::Farbfeld => Some(ImgFmt::Farbfeld),
            ImageFormat::Qoi => Some(ImgFmt::Qoi),
            _ => None,
        },
        Err(_) => None,
    }
}

fn check_dims(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("image dimensions must be at least 1x1".to_string());
    }
    if w > MAX_DIM || h > MAX_DIM {
        return Err(format!("image dimensions exceed {MAX_DIM}px"));
    }
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err("image pixel count exceeds 64M".to_string());
    }
    Ok(())
}

fn arg_bytes(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<Vec<u8>> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    view_bytes(cx, frame.arg(i), what)
}

fn set_rval_string(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

/// 光栅解码 → RGBA8（指定格式走显式，缺省走嗅探；动画首帧，16 位截断）。
fn decode_raster(bytes: &[u8], fmt: Option<ImgFmt>) -> Result<(ImgFmt, image::RgbaImage), String> {
    let fmt = match fmt {
        Some(f) => f,
        None => sniff(bytes).ok_or_else(|| "cannot detect image format".to_string())?,
    };
    let ifmt = match fmt {
        ImgFmt::Png => ImageFormat::Png,
        ImgFmt::Jpeg => ImageFormat::Jpeg,
        ImgFmt::Gif => ImageFormat::Gif,
        ImgFmt::WebP => ImageFormat::WebP,
        ImgFmt::Tiff => ImageFormat::Tiff,
        ImgFmt::Tga => ImageFormat::Tga,
        ImgFmt::Bmp => ImageFormat::Bmp,
        ImgFmt::Ico => ImageFormat::Ico,
        ImgFmt::Hdr => ImageFormat::Hdr,
        ImgFmt::Exr => ImageFormat::OpenExr,
        ImgFmt::Pnm => ImageFormat::Pnm,
        ImgFmt::Farbfeld => ImageFormat::Farbfeld,
        ImgFmt::Qoi => ImageFormat::Qoi,
        ImgFmt::Svg | ImgFmt::Jxl => {
            return Err("internal: vector format reached raster decoder".to_string());
        }
    };
    let img = image::load_from_memory_with_format(bytes, ifmt)
        .map_err(|e| format!("decode failed: {e}"))?;
    let rgba = img.to_rgba8();
    check_dims(rgba.width(), rgba.height())?;
    Ok((fmt, rgba))
}

/// SVG/SVGZ 光栅化（`usvg` 自动 gzip 识别；premultiplied 解回直色，透明零除保 0）。
fn decode_svg(bytes: &[u8], scale: f64) -> Result<image::RgbaImage, String> {
    if !(scale.is_finite() && scale > 0.0 && scale <= 32.0) {
        return Err("svg scale must be within (0, 32]".to_string());
    }
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|e| format!("svg parse failed: {e}"))?;
    let size = tree.size();
    let w = ((size.width() as f64) * scale).round() as u32;
    let h = ((size.height() as f64) * scale).round() as u32;
    check_dims(w.max(1), h.max(1))?;
    let w = w.max(1);
    let h = h.max(1);
    let mut pix =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| "cannot allocate pixmap".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32),
        &mut pix.as_mut(),
    );
    let raw = pix.take();
    let mut out = Vec::with_capacity(raw.len());
    for px in raw.chunks_exact(4) {
        let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3]);
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            out.push(((r * 255 + (a as u32) / 2) / (a as u32)).min(255) as u8);
            out.push(((g * 255 + (a as u32) / 2) / (a as u32)).min(255) as u8);
            out.push(((b * 255 + (a as u32) / 2) / (a as u32)).min(255) as u8);
            out.push(a);
        }
    }
    image::RgbaImage::from_raw(w, h, out).ok_or_else(|| "pixmap reshape failed".to_string())
}

/// JXL 解码（首帧；动画上游未实现，`JxlDecoder` 即单帧面）。
fn decode_jxl(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    let dec = jxl_oxide::integration::JxlDecoder::new(Cursor::new(bytes))
        .map_err(|e| format!("jxl init failed: {e}"))?;
    let img = image::DynamicImage::from_decoder(dec).map_err(|e| format!("jxl decode failed: {e}"))?;
    let rgba = img.to_rgba8();
    check_dims(rgba.width(), rgba.height())?;
    Ok(rgba)
}

/// `__wjs_image_info(bytes, format?)` → `{format,width,height,mime}` JSON。
pub unsafe extern "C" fn image_info(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, "WinterJS.image.info") else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, "TypeError: WinterJS.image.info requires non-empty bytes");
        return false;
    }
    let fmt = if frame.argc() > 1 {
        let s = value_to_string(&mut cx, frame.arg(1));
        if s.is_empty() || s == "undefined" {
            sniff(&bytes)
        } else {
            parse_format(&s)
        }
    } else {
        sniff(&bytes)
    };
    let Some(fmt) = fmt else {
        report_error(&mut cx, "TypeError: unsupported image format (dds/avif/pcx have no decoder)");
        return false;
    };
    let dims: Option<(u32, u32)> = match fmt {
        ImgFmt::Svg => resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default())
            .ok()
            .map(|t| (t.size().width() as u32, t.size().height() as u32)),
        ImgFmt::Jxl => decode_jxl(&bytes).ok().map(|im| (im.width(), im.height())),
        _ => {
            let ifmt = match fmt {
                ImgFmt::Png => ImageFormat::Png,
                ImgFmt::Jpeg => ImageFormat::Jpeg,
                ImgFmt::Gif => ImageFormat::Gif,
                ImgFmt::WebP => ImageFormat::WebP,
                ImgFmt::Tiff => ImageFormat::Tiff,
                ImgFmt::Tga => ImageFormat::Tga,
                ImgFmt::Bmp => ImageFormat::Bmp,
                ImgFmt::Ico => ImageFormat::Ico,
                ImgFmt::Hdr => ImageFormat::Hdr,
                ImgFmt::Exr => ImageFormat::OpenExr,
                ImgFmt::Pnm => ImageFormat::Pnm,
                ImgFmt::Farbfeld => ImageFormat::Farbfeld,
                ImgFmt::Qoi => ImageFormat::Qoi,
                _ => unreachable!(),
            };
            image::ImageReader::with_format(Cursor::new(&bytes), ifmt)
                .into_dimensions()
                .ok()
        }
    };
    let Some((w, h)) = dims else {
        report_error(&mut cx, "TypeError: cannot read image dimensions");
        return false;
    };
    if let Err(e) = check_dims(w.max(1), h.max(1)) {
        report_error(&mut cx, &format!("RangeError: {e}"));
        return false;
    }
    let json = serde_json::json!({
        "format": fmt_name(fmt),
        "width": w,
        "height": h,
        "mime": fmt_mime(fmt),
    })
    .to_string();
    set_rval_string(&mut cx, &frame, &json);
    true
}

/// `__wjs_image_pixels(bytes, format?)` → RGBA8 Uint8Array（元信息走 `image_info`）。
pub unsafe extern "C" fn image_pixels(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, "WinterJS.image.decode") else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, "TypeError: WinterJS.image.decode requires non-empty bytes");
        return false;
    }
    let (fmt, scale) = if frame.argc() > 1 {
        let s = value_to_string(&mut cx, frame.arg(1));
        if s.is_empty() || s == "undefined" {
            (None, 1.0)
        } else if let Some(f) = parse_format(&s) {
            (Some(f), 1.0)
        } else {
            // 数字形第二参 = svg scale 速记（`decode(bytes, 2)`）。
            match s.parse::<f64>() {
                Ok(v) => (None, v),
                Err(_) => {
                    report_error(&mut cx, "TypeError: unsupported image format (dds/avif/pcx have no decoder)");
                    return false;
                }
            }
        }
    } else {
        (None, 1.0)
    };
    let fmt = match fmt {
        Some(f) => f,
        None => match sniff(&bytes) {
            Some(f) => f,
            None => {
                report_error(&mut cx, "TypeError: cannot detect image format");
                return false;
            }
        },
    };
    // svg scale 可经第三参覆盖。
    let scale = if frame.argc() > 2 {
        let s = value_to_string(&mut cx, frame.arg(2));
        s.parse::<f64>().unwrap_or(scale)
    } else {
        scale
    };
    let rgba = match fmt {
        ImgFmt::Svg => match decode_svg(&bytes, scale) {
            Ok(im) => im,
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: {e}"));
                return false;
            }
        },
        ImgFmt::Jxl => match decode_jxl(&bytes) {
            Ok(im) => im,
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: {e}"));
                return false;
            }
        },
        _ => match decode_raster(&bytes, Some(fmt)) {
            Ok((_, im)) => im,
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: {e}"));
                return false;
            }
        },
    };
    if !set_rval_bytes(&mut cx, &frame, rgba.as_raw()) {
        return false;
    }
    true
}

/// `__wjs_image_encode(pixels, width, height, format, optionsJson)` → Uint8Array。
pub unsafe extern "C" fn image_encode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.image.encode";
    if frame.argc() < 4 {
        report_error(&mut cx, &format!("TypeError: {what} requires (pixels, width, height, format)"));
        return false;
    }
    let Some(px) = view_bytes(&mut cx, frame.arg(0), what) else {
        return false;
    };
    let (w, h) = (
        value_to_string(&mut cx, frame.arg(1)).parse::<u32>().unwrap_or(0),
        value_to_string(&mut cx, frame.arg(2)).parse::<u32>().unwrap_or(0),
    );
    let fmt_s = value_to_string(&mut cx, frame.arg(3));
    let Some(fmt) = parse_format(&fmt_s) else {
        report_error(&mut cx, "TypeError: unsupported image format (dds/avif/pcx/svg/jxl have no encoder)");
        return false;
    };
    if let Err(e) = check_dims(w, h) {
        report_error(&mut cx, &format!("RangeError: {e}"));
        return false;
    }
    if px.len() as u64 != (w as u64) * (h as u64) * 4 {
        report_error(&mut cx, "RangeError: pixel data length must equal width*height*4 (RGBA8)");
        return false;
    }
    let opts: serde_json::Value = if frame.argc() > 4 {
        let s = value_to_string(&mut cx, frame.arg(4));
        if s.is_empty() || s == "undefined" {
            serde_json::Value::Null
        } else {
            match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(_) => {
                    report_error(&mut cx, &format!("TypeError: {what} options must be an object"));
                    return false;
                }
            }
        }
    } else {
        serde_json::Value::Null
    };
    let opt = |k: &str| opts.get(k).unwrap_or(&serde_json::Value::Null).clone();
    let mut buf: Vec<u8> = Vec::new();
    let enc: Result<(), String> = (|| {
        let rgba = image::RgbaImage::from_raw(w, h, px.clone())
            .ok_or_else(|| "pixel reshape failed".to_string())?;
        match fmt {
            ImgFmt::Png => {
                use image::codecs::png::{CompressionType, FilterType, PngEncoder};
                let c = match opt("compression") {
                    serde_json::Value::Null => CompressionType::default(),
                    serde_json::Value::String(s) => match s.as_str() {
                        "default" => CompressionType::Default,
                        "fast" => CompressionType::Fast,
                        "best" => CompressionType::Best,
                        "uncompressed" => CompressionType::Uncompressed,
                        _ => return Err("png compression must be default/fast/best/uncompressed or 1..9".to_string()),
                    },
                    serde_json::Value::Number(n) => {
                        let f = n.as_f64().unwrap_or(0.0);
                        if f.fract() != 0.0 || !(1.0..=9.0).contains(&f) {
                            return Err("png compression level must be an integer within 1..9".to_string());
                        }
                        CompressionType::Level(f as u8)
                    }
                    _ => return Err("png compression must be default/fast/best/uncompressed or 1..9".to_string()),
                };
                let f = match opt("filter") {
                    serde_json::Value::Null => FilterType::default(),
                    serde_json::Value::String(s) => match s.as_str() {
                        "none" => FilterType::NoFilter,
                        "sub" => FilterType::Sub,
                        "up" => FilterType::Up,
                        "avg" => FilterType::Avg,
                        "paeth" => FilterType::Paeth,
                        "adaptive" => FilterType::Adaptive,
                        _ => return Err("png filter must be none/sub/up/avg/paeth/adaptive".to_string()),
                    },
                    _ => return Err("png filter must be none/sub/up/avg/paeth/adaptive".to_string()),
                };
                PngEncoder::new_with_quality(&mut buf, c, f)
                    .write_image(
                        rgba.as_raw(),
                        w,
                        h,
                        ExtendedColorType::Rgba8,
                    )
                    .map_err(|e| format!("png encode failed: {e}"))?;
            }
            ImgFmt::Jpeg => {
                use image::codecs::jpeg::JpegEncoder;
                let q = match opt("quality") {
                    serde_json::Value::Null => 75,
                    serde_json::Value::Number(n) => {
                        let f = n.as_f64().unwrap_or(0.0);
                        if f.fract() != 0.0 || !(1.0..=100.0).contains(&f) {
                            return Err("jpeg quality must be an integer within 1..100".to_string());
                        }
                        f as u8
                    }
                    _ => return Err("jpeg quality must be an integer within 1..100".to_string()),
                };
                JpegEncoder::new_with_quality(&mut buf, q)
                    .encode_image(&image::DynamicImage::ImageRgba8(rgba))
                    .map_err(|e| format!("jpeg encode failed: {e}"))?;
            }
            ImgFmt::Gif => {
                use image::codecs::gif::GifEncoder;
                let speed = match opt("speed") {
                    serde_json::Value::Null => None,
                    serde_json::Value::Number(n) => {
                        let f = n.as_f64().unwrap_or(0.0);
                        if f.fract() != 0.0 || !(1.0..=30.0).contains(&f) {
                            return Err("gif speed must be an integer within 1..30".to_string());
                        }
                        Some(f as i32)
                    }
                    _ => return Err("gif speed must be an integer within 1..30".to_string()),
                };
                let mut enc = match speed {
                    Some(s) => GifEncoder::new_with_speed(&mut buf, s),
                    None => GifEncoder::new(&mut buf),
                };
                match opt("repeat") {
                    serde_json::Value::Null => {}
                    serde_json::Value::Number(n) => {
                        let f = n.as_f64().unwrap_or(-1.0);
                        if f.fract() != 0.0 || f < 0.0 || f > 65535.0 {
                            return Err("gif repeat must be 0 (infinite) or within 1..65535".to_string());
                        }
                        let r = if f == 0.0 { Repeat::Infinite } else { Repeat::Finite(f as u16) };
                        enc.set_repeat(r).map_err(|e| format!("gif encode failed: {e}"))?;
                    }
                    _ => return Err("gif repeat must be 0 (infinite) or within 1..65535".to_string()),
                }
                enc.encode(
                    rgba.as_raw(),
                    w,
                    h,
                    ExtendedColorType::Rgba8,
                )
                .map_err(|e| format!("gif encode failed: {e}"))?;
            }
            ImgFmt::WebP => {
                // image-webp 只做 VP8L 无损：无质量参数（记档）。
                use image::codecs::webp::WebPEncoder;
                WebPEncoder::new_lossless(&mut buf)
                    .encode(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("webp encode failed: {e}"))?;
            }
            ImgFmt::Tiff => {
                use image::codecs::tiff::TiffEncoder;
                let mut cur = Cursor::new(&mut buf);
                TiffEncoder::new(&mut cur)
                    .write_image(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("tiff encode failed: {e}"))?;
            }
            ImgFmt::Bmp => {
                use image::codecs::bmp::BmpEncoder;
                BmpEncoder::new(&mut buf)
                    .encode(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("bmp encode failed: {e}"))?;
            }
            ImgFmt::Ico => {
                if w > 256 || h > 256 {
                    return Err("ico dimensions must not exceed 256x256".to_string());
                }
                use image::codecs::ico::{IcoEncoder, IcoFrame};
                use image::codecs::png::PngEncoder;
                let mut png: Vec<u8> = Vec::new();
                PngEncoder::new(&mut png)
                    .write_image(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("ico encode failed: {e}"))?;
                let frame = IcoFrame::with_encoded(png, w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("ico encode failed: {e}"))?;
                IcoEncoder::new(&mut buf)
                    .encode_images(&[frame])
                    .map_err(|e| format!("ico encode failed: {e}"))?;
            }
            ImgFmt::Tga => {
                use image::codecs::tga::TgaEncoder;
                TgaEncoder::new(&mut buf)
                    .encode(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("tga encode failed: {e}"))?;
            }
            ImgFmt::Hdr => {
                use image::codecs::hdr::HdrEncoder;
                let rgb: Vec<image::Rgb<f32>> = rgba
                    .pixels()
                    .map(|p| {
                        let c = p.0;
                        image::Rgb([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0])
                    })
                    .collect();
                HdrEncoder::new(&mut buf)
                    .encode(&rgb, w as usize, h as usize)
                    .map_err(|e| format!("hdr encode failed: {e}"))?;
            }
            ImgFmt::Exr => {
                // exr 只收 f32（Rgba32F/Rgb32F/Rgba16F 系）：u8 归一化上转。
                use image::codecs::openexr::OpenExrEncoder;
                let f: Vec<f32> = rgba
                    .as_raw()
                    .iter()
                    .map(|&v| v as f32 / 255.0)
                    .collect();
                let mut fbytes = Vec::with_capacity(f.len() * 4);
                for v in &f {
                    fbytes.extend_from_slice(&v.to_ne_bytes());
                }
                let mut cur = Cursor::new(&mut buf);
                OpenExrEncoder::new(&mut cur)
                    .write_image(&fbytes, w, h, ExtendedColorType::Rgba32F)
                    .map_err(|e| format!("exr encode failed: {e}"))?;
            }
            ImgFmt::Qoi => {
                use image::codecs::qoi::QoiEncoder;
                QoiEncoder::new(&mut buf)
                    .write_image(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|e| format!("qoi encode failed: {e}"))?;
            }
            ImgFmt::Farbfeld => {
                // farbfeld 是 16 位通道（8 字节/像素，native endian）：u8 上转。
                use image::codecs::farbfeld::FarbfeldEncoder;
                let mut wide = Vec::with_capacity(px.len() * 2);
                for &v in rgba.as_raw() {
                    wide.extend_from_slice(&((v as u16 * 257).to_ne_bytes()));
                }
                FarbfeldEncoder::new(&mut buf)
                    .encode(&wide, w, h)
                    .map_err(|e| format!("farbfeld encode failed: {e}"))?;
            }
            ImgFmt::Pnm => {
                use image::codecs::pnm::{PnmEncoder, PnmSubtype, SampleEncoding};
                let sub = match opt("subtype") {
                    serde_json::Value::Null => "ppm".to_string(),
                    serde_json::Value::String(s) => s.to_ascii_lowercase(),
                    _ => return Err("pnm subtype must be ppm/pgm/pbm/pam".to_string()),
                };
                let binary = match opt("encoding") {
                    serde_json::Value::Null => true,
                    serde_json::Value::String(s) => match s.to_ascii_lowercase().as_str() {
                        "binary" => true,
                        "ascii" => false,
                        _ => return Err("pnm encoding must be binary/ascii".to_string()),
                    },
                    _ => return Err("pnm encoding must be binary/ascii".to_string()),
                };
                let se = if binary { SampleEncoding::Binary } else { SampleEncoding::Ascii };
                let mut enc = PnmEncoder::new(&mut buf).with_subtype(match sub.as_str() {
                    "ppm" => PnmSubtype::Pixmap(se),
                    "pgm" => PnmSubtype::Graymap(se),
                    "pbm" => PnmSubtype::Bitmap(se),
                    "pam" => PnmSubtype::ArbitraryMap,
                    _ => return Err("pnm subtype must be ppm/pgm/pbm/pam".to_string()),
                });
                let dynimg = || image::DynamicImage::ImageRgba8(rgba.clone());
                match sub.as_str() {
                    "pgm" => {
                        let l = dynimg().to_luma8();
                        enc.encode(l.as_raw().as_slice(), w, h, ExtendedColorType::L8)
                            .map_err(|e| format!("pnm encode failed: {e}"))?;
                    }
                    "pbm" => {
                        let l = dynimg().to_luma8();
                        let bw: Vec<u8> = l.as_raw().iter().map(|&v| if v >= 128 { 255 } else { 0 }).collect();
                        enc.encode(bw.as_slice(), w, h, ExtendedColorType::L8)
                            .map_err(|e| format!("pnm encode failed: {e}"))?;
                    }
                    _ => {
                        // ppm 要 RGB 三通道（RGBA 直喂即断言 abort，见 4.231 族），
                        // pam（ArbitraryMap）才收 RGBA。
                        if sub.as_str() == "pam" {
                            enc.encode(rgba.as_raw().as_slice(), w, h, ExtendedColorType::Rgba8)
                                .map_err(|e| format!("pnm encode failed: {e}"))?;
                        } else {
                            let rgb: Vec<u8> = rgba
                                .pixels()
                                .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
                                .collect();
                            enc.encode(rgb.as_slice(), w, h, ExtendedColorType::Rgb8)
                                .map_err(|e| format!("pnm encode failed: {e}"))?;
                        }
                    }
                }
            }
            ImgFmt::Svg | ImgFmt::Jxl => {
                return Err("encoding not supported for svg/jxl (decode only)".to_string());
            }
        }
        Ok(())
    })();
    if let Err(e) = enc {
        let is_range = e.contains("must be") || e.contains("exceed") || e.contains("length");
        report_error(&mut cx, &format!("{}: {e}", if is_range { "RangeError" } else { "TypeError" }));
        return false;
    }
    if !set_rval_bytes(&mut cx, &frame, &buf) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_names_roundtrip() {
        for (name, want) in [
            ("png", ImgFmt::Png),
            ("jpg", ImgFmt::Jpeg),
            ("jpeg", ImgFmt::Jpeg),
            ("tif", ImgFmt::Tiff),
            ("ff", ImgFmt::Farbfeld),
            ("ppm", ImgFmt::Pnm),
            ("pam", ImgFmt::Pnm),
            ("svgz", ImgFmt::Svg),
            ("jxl", ImgFmt::Jxl),
        ] {
            assert_eq!(parse_format(name), Some(want), "{name}");
        }
        assert_eq!(parse_format("dds"), None);
        assert_eq!(parse_format("avif"), None);
        assert_eq!(parse_format(""), None);
        assert_eq!(fmt_name(ImgFmt::Qoi), "qoi");
        assert_eq!(fmt_mime(ImgFmt::Png), "image/png");
    }

    #[test]
    fn sniff_magics() {
        assert_eq!(
            sniff(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
            Some(ImgFmt::Png)
        );
        assert_eq!(sniff(b"<svg xmlns='x'/>"), Some(ImgFmt::Svg));
        assert_eq!(sniff(&[0x1f, 0x8b, 0x08, 0x00]), Some(ImgFmt::Svg));
        assert_eq!(sniff(&[0xff, 0x0a, 0, 0]), Some(ImgFmt::Jxl));
        assert_eq!(sniff(b"BM........"), Some(ImgFmt::Bmp));
        assert_eq!(sniff(b"nope"), None);
        assert_eq!(sniff(&[]), None);
    }

    #[test]
    fn dim_guards() {
        assert!(check_dims(1, 1).is_ok());
        assert!(check_dims(0, 1).is_err());
        assert!(check_dims(16385, 1).is_err());
        assert!(check_dims(8192, 8193).is_err());
    }

    #[test]
    fn jxl_header_stub_behavior() {
        // 头合法但帧截断：上游流式解码按声明尺寸零填（240x135），不报错——
        // 成功路径仍缺真 fixture（记档跟进）；非 jxl 字节必须干净 Err，不 panic。
        let stub = [
            0xff, 0x0a, 0x30, 0x54, 0x10, 0x09, 0x08, 0x06, 0x01, 0x00, 0x78, 0x00, 0x4b, 0x38,
            0x41, 0x3c, 0xb6, 0x3a, 0x51, 0xfe, 0x00, 0x47, 0x1e, 0xa0, 0x85, 0xb8, 0x27, 0x1a,
            0x48, 0x45, 0x84, 0x1b, 0x71, 0x4f, 0xa8, 0x3e, 0x8e, 0x30, 0x03, 0x92, 0x84, 0x01,
        ];
        assert_eq!(sniff(&stub), Some(ImgFmt::Jxl));
        let im = decode_jxl(&stub).expect("header-valid stub decodes (zero-filled)");
        assert_eq!((im.width(), im.height()), (240, 135));
        assert!(decode_jxl(b"nope").is_err());
        assert!(decode_jxl(&[]).is_err());
    }
}

//! 启动 banner（2026-09-28，用户拍板）：每次运行首行走 stderr。
//!
//! - 图形终端（Kitty/iTerm 系）：`assets/logo.jxl`（`jxl-oxide` 解码，纯 Rust）
//!   + `assets/winterjs.svg`（`resvg` 光栅化）经对应转义协议输出，之后跟版本行。
//!   素材史：`logo.avif` 因纯 Rust 解码无轮子（`image/avif` 只管编码，见 §14
//!   `viuer` 条）经 `cjxl` 转码为同 artwork 的 `logo.jxl`（`sips` 先验 alpha）。
//! - 其余终端：纯 ASCII 雪花 + `w i n t e r j s <ver>`（字节恒 <0x80，单测钉住）。
//! - 关闭：`-hide_banner`（破例单横杠 + 下划线，见 `cli::rewrite_banner_flag`）或
//!   `--hide_banner`；stderr 非 TTY 自动跳过（管道/CI 零噪音）；`--completions`/
//!   `--man` 不打印（纯机器输出）。
//!
//! 约束（实测结论，见 `docs/dependencies.md` §14 `viuer` 条）：
//! - 检测只读 env，绝不发终端查询（viuer 的 Kitty 检测往 stdout 写 `\x1b_G…` 并从
//!   stdin 读应答——会污染 stdout 数据通道 + 吃掉 REPL 首键，启动期禁用）。
//! - 发射走 stderr（viuer 的 `Printer` 未导出，`print` 锁 stdout）。
//! - 任何失败静默回落 ASCII（banner 永不炸启动）；禁把用户数据打进输出（只版本号）。

use std::io::Write as _;

/// 内嵌素材（`assets/`，`include_bytes!/str!` 零新文件）。
const LOGO_JXL: &[u8] = include_bytes!("../assets/logo.jxl");
const WORDMARK_SVG: &str = include_str!("../assets/winterjs.svg");

/// SVG 深色字（浅底用）→ 深底替换色（与 SVG 内 `@media` 同值，`COLORFGBG` 判定）。
const DARK_FILL: &str = "#16233a";
const LIGHT_FILL: &str = "#f2f5f9";

/// 图形展示列数（avif 方图窄些，wordmark 宽些；行数按 cell 1:2 换算）。
const AVIF_COLS: u32 = 28;
const SVG_COLS: u32 = 44;
/// logo 预缩像素宽（1261 原图太大，转 PNG 发射前先缩，省转义字节）。
const LOGO_PX_W: u32 = 384;
/// wordmark 渲染像素宽（矢量直接按此 render，保证清晰）。
const SVG_PX_W: u32 = 544;
/// Kitty 分块转义每块 base64 字符数（viuer 同值）。
const KITTY_CHUNK: usize = 4096;

/// 纯 ASCII 雪花（7 行；每行 ≤17 列，左对齐块；`w i n t e r…` 版本行由代码另拼）。
const ASCII_FLAKE: &[&str] = &[
    "        *        ",
    "   \\\\   |   /    ",
    "    \\\\  |  /     ",
    "------  *  ------",
    "    /  |  \\\\     ",
    "   /   |   \\\\    ",
    "        *        ",
];

/// 图形协议（env 检测，无终端查询）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Kitty,
    Iterm,
}

/// 显示模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    Ascii,
    Graphics(Proto),
}

fn env_is(key: &str) -> bool {
    std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false)
}

fn env_contains(key: &str, needles: &[&str]) -> bool {
    match std::env::var(key) {
        Ok(v) => needles.iter().any(|n| v.contains(n)),
        Err(_) => false,
    }
}

/// Kitty 系 env（WezTerm/Ghostty/Konsole 走 Kitty 协议；查询式检测禁用，见模块注）。
fn kitty_capable() -> bool {
    if env_is("KITTY_WINDOW_ID") || env_is("WEZTERM_PANE") || env_is("KONSOLE_VERSION") {
        return true;
    }
    if std::env::var("TERM").map(|v| v == "xterm-kitty").unwrap_or(false) {
        return true;
    }
    env_contains("TERM_PROGRAM", &["kitty", "Kitty", "WezTerm", "wezterm", "ghostty", "Ghostty"])
}

/// iTerm 系 env（名单照抄 viuer `check_iterm_support` 源码，env 只读无查询）。
fn iterm_capable() -> bool {
    if env_contains("TERM_PROGRAM", &["iTerm", "WezTerm", "mintty", "rio", "WarpTerminal"]) {
        return true;
    }
    if env_contains("LC_TERMINAL", &["iTerm", "WezTerm", "mintty", "rio"]) {
        return true;
    }
    env_is("KONSOLE_VERSION")
}

/// 协议判定（Kitty 优先；iTerm 名单与 Kitty 重叠的 WezTerm/Konsole 走 Kitty）。
pub fn detect() -> Option<Proto> {
    if kitty_capable() {
        Some(Proto::Kitty)
    } else if iterm_capable() {
        Some(Proto::Iterm)
    } else {
        None
    }
}

/// 背景是否浅色（`COLORFGBG="fg;bg"`，bg 7/15/231/255 判浅；缺省按深底处理——
/// dev 终端深底占优，且未知时两边误伤对称，取常见侧）。
pub fn bg_is_light() -> bool {
    match std::env::var("COLORFGBG") {
        Ok(v) => v
            .rsplit(';')
            .next()
            .and_then(|bg| bg.trim().parse::<u32>().ok())
            .map(|bg| matches!(bg, 7 | 15 | 231 | 255))
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// stderr 是否 TTY（std，原生，无新依赖）。
pub fn stderr_is_tty() -> bool {
    use std::io::IsTerminal as _;
    std::io::stderr().is_terminal()
}

/// 总决策（纯函数化入参，便于单测；env 只在检测层读）。
pub fn decide(hide: bool, stderr_tty: bool) -> Mode {
    if hide || !stderr_tty {
        return Mode::Hidden;
    }
    match detect() {
        Some(p) => Mode::Graphics(p),
        None => Mode::Ascii,
    }
}

/// 版本行（ASCII 与图形共用收尾）。
pub fn version_line() -> String {
    format!("   w i n t e r j s  {}", env!("CARGO_PKG_VERSION"))
}

/// ASCII banner 全文（纯函数，单测 + 黑盒断言同一份）。
pub fn ascii_text() -> String {
    let mut out = String::new();
    for line in ASCII_FLAKE {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&version_line());
    out.push('\n');
    out
}

/// PNG 编码（DynamicImage→PNG 字节；两协议共用载荷）。
fn png_of(img: &image::DynamicImage) -> Option<Vec<u8>> {
    use image::ImageEncoder as _;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(buf)
}

/// logo 解码 + 预缩（`jxl-oxide` 的 `image` 集成直出 DynamicImage，含 alpha）。
fn decode_logo() -> Option<image::DynamicImage> {
    let cursor = std::io::Cursor::new(LOGO_JXL);
    let decoder = jxl_oxide::integration::JxlDecoder::new(cursor).ok()?;
    image::DynamicImage::from_decoder(decoder)
        .ok()
        .map(|img| img.thumbnail(LOGO_PX_W, LOGO_PX_W))
}

/// wordmark 光栅化（矢量按目标宽直接 render；无字体/解析失败即 None→ASCII 回落）。
fn render_wordmark() -> Option<image::DynamicImage> {
    let owned;
    let src = if bg_is_light() {
        WORDMARK_SVG
    } else {
        owned = WORDMARK_SVG.replace(DARK_FILL, LIGHT_FILL);
        &owned
    };
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(src, &opt).ok()?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return None;
    }
    let scale = SVG_PX_W as f32 / size.width();
    let (w, h) = (SVG_PX_W, (size.height() * scale).max(1.0) as u32);
    let mut pix = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pix.as_mut(),
    );
    let raw = pix.data().to_vec();
    let buf = image::RgbaImage::from_raw(w, h, raw)?;
    Some(image::DynamicImage::ImageRgba8(buf))
}

/// cell 行数换算（cell 高≈2 倍宽，viuer `find_best_fit` 同口径）。
fn cell_rows(cols: u32, w_px: u32, h_px: u32) -> u32 {
    if w_px == 0 {
        return 1;
    }
    ((cols as u64 * h_px as u64) / w_px as u64 / 2).max(1) as u32
}

/// Kitty 转义序列（帧形照抄 viuer `print_remote` 源码：首块 `f=100,a=T,t=d` +
/// 4096 分块，`m=1…m=0`；目标改 stderr，载荷改 PNG）。
/// 纯函数：单测断言帧头/帧尾/base64 往返。
pub fn kitty_seq(png: &[u8], w_px: u32, h_px: u32, cols: u32) -> Vec<u8> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let rows = cell_rows(cols, w_px, h_px);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(KITTY_CHUNK).collect();
    let mut out = Vec::new();
    if chunks.is_empty() {
        // 空载荷也发一帧空块（m=0），对端清状态。
        out.extend_from_slice(
            format!("\x1b_Gf=100,a=T,t=d,s={w_px},v={h_px},c={cols},r={rows},m=0;").as_bytes(),
        );
        out.extend_from_slice(b"\x1b\\");
        return out;
    }
    for (i, c) in chunks.iter().enumerate() {
        let m = if i + 1 == chunks.len() { 0 } else { 1 };
        if i == 0 {
            out.extend_from_slice(
                format!("\x1b_Gf=100,a=T,t=d,s={w_px},v={h_px},c={cols},r={rows},m={m};")
                    .as_bytes(),
            );
        } else {
            out.extend_from_slice(format!("\x1b_Gm={m};").as_bytes());
        }
        out.extend_from_slice(c);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

/// iTerm2 转义序列（帧形照抄 viuer `print_buffer` 源码：`1337;File=inline=1`，
/// 宽高裸数 = cells，viuer `find_best_fit` 同口径）。
/// 纯函数：单测断言帧头/帧尾/base64 往返。
pub fn iterm_seq(png: &[u8], cols: u32, rows: u32) -> Vec<u8> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    format!("\x1b]1337;File=inline=1;preserveAspectRatio=1;size={};width={cols};height={rows}:{b64}\x07", png.len()).into_bytes()
}

/// 图形 banner 发射（stderr；任一步失败即 None→调用方回落 ASCII）。
fn print_graphics(p: Proto) -> bool {
    let mut err = std::io::stderr().lock();
    // avif 主图 + svg 字牌，任一失败整单回落（半幅 banner 不如全 ASCII）。
    let logo = decode_logo().and_then(|img| {
        let (w, h) = (img.width(), img.height());
        png_of(&img).map(|png| (png, w, h, AVIF_COLS))
    });
    let word = render_wordmark().and_then(|img| {
        let (w, h) = (img.width(), img.height());
        png_of(&img).map(|png| (png, w, h, SVG_COLS))
    });
    let (Some((lpng, lw, lh, lcols)), Some((wpng, ww, wh, wcols))) = (logo, word) else {
        return false;
    };
    let ok = match p {
        Proto::Kitty => {
            let a = kitty_seq(&lpng, lw, lh, lcols);
            let b = kitty_seq(&wpng, ww, wh, wcols);
            err.write_all(&a).and_then(|_| err.write_all(&b)).is_ok()
        }
        Proto::Iterm => {
            let a = iterm_seq(&lpng, lcols, cell_rows(lcols, lw, lh));
            let b = iterm_seq(&wpng, wcols, cell_rows(wcols, ww, wh));
            err.write_all(&a).and_then(|_| err.write_all(&b)).is_ok()
        }
    };
    if !ok {
        return false;
    }
    writeln!(err, "{}", version_line()).is_ok()
}

/// 启动 banner 入口（尽力而为：永不报错，永不 panic，永不碰 stdout）。
pub fn print_startup(hide: bool) {
    match decide(hide, stderr_is_tty()) {
        Mode::Hidden => {}
        Mode::Ascii => {
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(ascii_text().as_bytes());
            let _ = err.flush();
        }
        Mode::Graphics(p) => {
            if !print_graphics(p) {
                let mut err = std::io::stderr().lock();
                let _ = err.write_all(ascii_text().as_bytes());
                let _ = err.flush();
            } else {
                let _ = std::io::stderr().flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn ascii_is_pure_ascii() {
        let t = ascii_text();
        assert!(t.bytes().all(|b| b < 0x80), "banner must stay pure ASCII");
        assert!(t.contains("w i n t e r j s"), "{t}");
        assert!(t.contains(env!("CARGO_PKG_VERSION")), "{t}");
    }

    #[test]
    fn flake_rows_aligned() {
        // 雪花块左对齐等宽（行 0–6 等长 17 列）。
        for line in ASCII_FLAKE {
            assert_eq!(line.len(), 17, "{line:?}");
        }
    }

    #[test]
    fn kitty_frames_roundtrip() {
        let png = vec![0u8; 100];
        let seq = kitty_seq(&png, 10, 10, 8);
        assert!(seq.starts_with(b"\x1b_Gf=100,a=T"), "{seq:?}");
        assert!(seq.ends_with(b"\x1b\\"), "{seq:?}");
        // 载荷拼回即 base64 原文。
        let body: Vec<u8> = seq
            .split(|b| *b == b';')
            .skip(1)
            .flat_map(|part| {
                part.strip_suffix(b"\x1b\\").unwrap_or(part).to_vec()
            })
            .collect();
        let joined = String::from_utf8(body).unwrap().replace("\x1b_Gm=0;", "");
        use base64::Engine as _;
        assert_eq!(
            base64::engine::general_purpose::STANDARD.decode(joined).unwrap(),
            png
        );
        // 多块：m=1 开头。
        let big = vec![7u8; 100_000];
        let seq = kitty_seq(&big, 10, 10, 8);
        assert!(seq.windows(7).any(|w| w == b"\x1b_Gm=1;"), "multi-chunk");
        assert!(seq.ends_with(b"\x1b\\"));
    }

    #[test]
    fn iterm_frames_roundtrip() {
        let png = vec![1u8, 2, 3];
        let seq = iterm_seq(&png, 8, 4);
        let s = String::from_utf8(seq).unwrap();
        assert!(s.starts_with("\x1b]1337;File=inline=1;"), "{s}");
        assert!(s.ends_with("\x07"), "{s}");
        assert!(s.contains("width=8;height=4:"), "{s}");
        let b64 = s.rsplit(':').next().unwrap().trim_end_matches('\x07');
        use base64::Engine as _;
        assert_eq!(
            base64::engine::general_purpose::STANDARD.decode(b64).unwrap(),
            png
        );
    }

    #[test]
    fn decide_matrix() {
        // 纯入参矩阵（env 不参与此层）。
        assert_eq!(decide(true, true), Mode::Hidden);
        assert_eq!(decide(false, false), Mode::Hidden);
        // tty 下非 Hidden 即 Ascii/Graphics 二选一（本机 env 定）。
        assert_ne!(decide(false, true), Mode::Hidden);
    }

    #[test]
    #[serial]
    fn bg_parse() {
        // COLORFGBG 解析（串行单测内改全局 env，先存后还，§4.41）。
        let old = std::env::var("COLORFGBG").ok();
        // SAFETY: 单测串行（#[serial]），无其他线程读写 env。
        unsafe {
            std::env::set_var("COLORFGBG", "15;0");
        }
        assert!(!bg_is_light());
        // SAFETY: 同上。
        unsafe {
            std::env::set_var("COLORFGBG", "0;15");
        }
        assert!(bg_is_light());
        // SAFETY: 同上。
        unsafe {
            std::env::remove_var("COLORFGBG");
        }
        assert!(!bg_is_light());
        if let Some(v) = old {
            // SAFETY: 同上（恢复现场）。
            unsafe {
                std::env::set_var("COLORFGBG", v);
            }
        }
    }

    #[test]
    fn cell_rows_math() {
        assert_eq!(cell_rows(28, 1261, 1247), 13);
        assert_eq!(cell_rows(44, 680, 180), 5);
        assert_eq!(cell_rows(8, 0, 0), 1);
    }

    #[test]
    fn decode_and_render_real_assets() {
        // 真素材端到端（avif 解码 + svg 光栅 + PNG 编码；本机有系统字体）。
        let logo = decode_logo().expect("avif decodes");
        assert!(logo.width() <= LOGO_PX_W);
        let png = png_of(&logo).expect("png encodes");
        assert!(png.windows(8).any(|w| w == b"\x89PNG\r\n\x1a\n"), "png magic");
        let word = render_wordmark().expect("svg renders");
        assert_eq!(word.width(), SVG_PX_W);
        let _ = png_of(&word).expect("png encodes");
        // 转义序列可发射（载荷往返见上两单测）。
        assert!(!kitty_seq(&png, logo.width(), logo.height(), AVIF_COLS).is_empty());
    }
}

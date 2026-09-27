//! 启动 banner（2026-09-28，用户拍板）：每次运行首行走 stderr。
//!
//! - 图形终端（Kitty/iTerm 系）：`assets/logo.jxl`（`jxl-oxide` 解码，纯 Rust）
//!   + `assets/winterjs.svg`（`resvg` 光栅化）经对应转义协议输出，之后跟版本行。
//!   素材史：`logo.avif` 因纯 Rust 解码无轮子（`image/avif` 只管编码，见 §14
//!   `viuer` 条）经 `cjxl` 转码为同 artwork 的 `logo.jxl`（`sips` 先验 alpha）。
//! - 其余终端：用户定稿 ASCII art（❄/❆ 分隔线 + WINTER JS 块字，版本行居中）。
//! - 关闭：`-hide_banner`（单横杠破例，非 clap flag，见 `cli::strip_banner_flags`）；
//!   强制 ASCII：`-ascii_banner`（图形终端也走雪花，hide 优先）；stderr 非 TTY 自动跳过（管道/CI 零噪音）
//!   `--completions` / `--man` 不打印（纯机器输出）。
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

/// 横式 lockup（2026-09-28）：logo 与字牌并排等高，一张图一次发射。
/// 总高 4 行（logo 8 列 + 2 列缝 + 字牌 30 列 ≈ 40 列）；图形版不带版本号。
/// 取小值：部分终端忽略尺寸参数按像素原尺寸直出，像素本身取展示尺寸——
/// 参数 honor 走 cell 精确尺寸，参数被忽略走小像素兜底，两边都不炸。
const LOCKUP_ROWS: u32 = 4;
/// logo 像素边长（1261 原图拉齐正方形，1% 拉伸不可见）。
const LOGO_PX: u32 = 128;
/// 字牌像素高（矢量按高直接 render，保证清晰）。
const WORD_PX_H: u32 = 128;
/// 两件之间像素缝。
const GAP_PX: u32 = 16;
/// 两件之间展示列数。
const GAP_COLS: u32 = 2;
/// Kitty 分块转义每块 base64 字符数（viuer 同值）。
const KITTY_CHUNK: usize = 4096;

/// ASCII banner（用户定稿 2026-09-28）：❄/❆ 分隔线 + WINTER JS 块字。
/// 非纯 ASCII（块字符与雪花皆单 cell 宽，等宽终端下对齐）；版本行按最宽行居中。
const ASCII_ART: &[&str] = &[
    "❄     ·     ❆     ·     ❄     ·     ❆     ·     ❄     ·     ❆     ·     ❄",
    "██╗    ██╗██╗███╗   ██╗████████╗███████╗██████╗      ██╗███████╗",
    "██║    ██║██║████╗  ██║╚══██╔══╝██╔════╝██╔══██╗     ██║██╔════╝",
    "██║ █╗ ██║██║██╔██╗ ██║   ██║   █████╗  ██████╔╝     ██║███████╗",
    "██║███╗██║██║██║╚██╗██║   ██║   ██╔══╝  ██╔══██╗██   ██║╚════██║",
    "╚███╔███╔╝██║██║ ╚████║   ██║   ███████╗██║  ██║╚█████╔╝███████║",
    " ╚══╝╚══╝ ╚═╝╚═╝  ╚═══╝   ╚═╝   ╚══════╝╚═╝  ╚═╝ ╚════╝ ╚══════╝",
    "❆     ·     ❄     ·     ❆     ·     ❄     ·     ❆     ·     ❄     ·     ❆",
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

/// 总决策（纯函数化入参，便于单测；env 只在检测层读；hide 优先于 ascii）。
pub fn decide(hide: bool, ascii: bool, stderr_tty: bool) -> Mode {
    if hide || !stderr_tty {
        return Mode::Hidden;
    }
    if ascii {
        return Mode::Ascii;
    }
    match detect() {
        Some(p) => Mode::Graphics(p),
        None => Mode::Ascii,
    }
}

/// 版本行（ASCII 与图形共用收尾）。
pub fn version_line() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// ASCII banner 全文（纯函数，单测 + 黑盒断言同一份）。
pub fn ascii_text() -> String {
    let width = ASCII_ART.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let ver = version_line();
    let pad = width.saturating_sub(ver.chars().count()) / 2;
    let mut out = String::new();
    for line in ASCII_ART {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&" ".repeat(pad));
    out.push_str(&ver);
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

/// logo 解码 + 拉齐正方形（`jxl-oxide` 的 `image` 集成直出 DynamicImage，含 alpha；
/// 1% 拉伸不可见，lockup 拼版要求严丝合缝）。
fn decode_logo() -> Option<image::RgbaImage> {
    let cursor = std::io::Cursor::new(LOGO_JXL);
    let decoder = jxl_oxide::integration::JxlDecoder::new(cursor).ok()?;
    let img = image::DynamicImage::from_decoder(decoder).ok()?;
    Some(image::imageops::resize(
        &img.to_rgba8(),
        LOGO_PX,
        LOGO_PX,
        image::imageops::FilterType::Lanczos3,
    ))
}

/// wordmark 光栅化（矢量按目标高直接 render；无字体/解析失败即 None→ASCII 回落）。
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
    let scale = WORD_PX_H as f32 / size.height();
    let (w, h) = ((size.width() * scale).max(1.0) as u32, WORD_PX_H);
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

/// 横式 lockup 拼版：logo 左 + 字牌右，等高（`LOCKUP_ROWS` 行），透明底。
/// 任一素材失败即 None→调用方回落 ASCII（半幅不如全 ASCII）。
fn lockup() -> Option<image::DynamicImage> {
    let logo = decode_logo()?;
    let word = render_wordmark()?;
    let (lw, lh) = (logo.width(), logo.height());
    let (ww, wh) = (word.width(), word.height());
    let h = lh.max(wh).max(1);
    let w = lw + GAP_PX + ww;
    let mut canvas = image::RgbaImage::new(w, h);
    let y_logo = (h - lh) / 2;
    let y_word = (h - wh) / 2;
    image::imageops::overlay(&mut canvas, &logo, 0, y_logo as i64);
    image::imageops::overlay(&mut canvas, &word, (lw + GAP_PX) as i64, y_word as i64);
    Some(image::DynamicImage::ImageRgba8(canvas))
}

/// lockup 展示列数（logo 2×行 + 缝 + 字牌按比换算；cell 高≈2 倍宽）。
fn lockup_cols(word_px_w: u32, word_px_h: u32) -> u32 {
    LOCKUP_ROWS * 2 + GAP_COLS + cell_cols(LOCKUP_ROWS, word_px_w, word_px_h)
}

/// cell 列数换算（`cell_rows` 逆运算；整数截断，至少 1）。
fn cell_cols(rows: u32, w_px: u32, h_px: u32) -> u32 {
    if h_px == 0 {
        return 1;
    }
    ((rows as u64 * w_px as u64) / h_px as u64 * 2).max(1) as u32
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

/// 图形 banner 发射（stderr；lockup 任一步失败即 false→调用方回落 ASCII）。
fn print_graphics(p: Proto) -> bool {
    let mut err = std::io::stderr().lock();
    let Some(img) = lockup() else {
        return false;
    };
    let (w, h) = (img.width(), img.height());
    let Some(png) = png_of(&img) else {
        return false;
    };
    // 字牌像素宽按 lockup 实测回填列数（logo 2×行 + 缝 + 字牌换算）。
    let word_px_w = w.saturating_sub(LOGO_PX + GAP_PX);
    let cols = lockup_cols(word_px_w, h);
    let ok = match p {
        Proto::Kitty => err.write_all(&kitty_seq(&png, w, h, cols)).is_ok(),
        Proto::Iterm => err.write_all(&iterm_seq(&png, cols, LOCKUP_ROWS)).is_ok(),
    };
    if !ok {
        return false;
    }
    // 收尾换行：部分终端图片展示后光标停在行中，一个 `\n` 把光标送到下一行行首
    // （图形版无版本行，ASCII 版版本行另拼，见 `ascii_text`）。
    let _ = err.write_all(b"\n");
    let _ = err.flush();
    true
}

/// 启动 banner 入口（尽力而为：永不报错，永不 panic，永不碰 stdout）。
pub fn print_startup(hide: bool, ascii: bool) {
    match decide(hide, ascii, stderr_is_tty()) {
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
    fn ascii_art_shape() {
        // 用户定稿 art：不断言纯 ASCII（块字符/雪花），只钉形状与版本。
        let t = ascii_text();
        assert!(t.contains('❄') && t.contains('❆'), "{t}");
        assert!(t.contains("██╗") && t.contains('·'), "{t}");
        assert!(t.contains(env!("CARGO_PKG_VERSION")), "{t}");
        for line in t.lines() {
            assert!(!line.ends_with(' ') && !line.ends_with('\t'), "{line:?}");
            assert!(line.chars().count() <= 80, "{line:?}");
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
        // 纯入参矩阵（env 不参与此层）；hide 优先于 ascii。
        assert_eq!(decide(true, false, true), Mode::Hidden);
        assert_eq!(decide(true, true, true), Mode::Hidden);
        assert_eq!(decide(false, false, false), Mode::Hidden);
        assert_eq!(decide(false, true, true), Mode::Ascii);
        // tty 下非 Hidden 即 Ascii/Graphics 二选一（本机 env 定）。
        assert_ne!(decide(false, false, true), Mode::Hidden);
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
        assert_eq!(cell_rows(20, 1261, 1247), 9);
        assert_eq!(cell_rows(36, 680, 180), 4);
        assert_eq!(cell_rows(8, 0, 0), 1);
    }

    #[test]
    fn decode_and_render_real_assets() {
        // 真素材端到端（jxl 解码 + svg 光栅 + lockup 拼版 + PNG 编码；本机有系统字体）。
        let pic = lockup().expect("lockup composites");
        assert_eq!((pic.width(), pic.height()), (627, 128));
        let png = png_of(&pic).expect("png encodes");
        assert!(png.windows(8).any(|w| w == b"\x89PNG\r\n\x1a\n"), "png magic");
        // 展示列数：logo 8 + 缝 2 + 字牌 30 = 40。
        assert_eq!(lockup_cols(483, 128), 40);
        // 转义序列可发射（载荷往返见上两单测）。
        assert!(!kitty_seq(&png, pic.width(), pic.height(), 40).is_empty());
    }
}

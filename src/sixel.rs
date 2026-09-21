//! sixel 그래픽: 터미널 지원 감지, 색 줄이기, 인코딩.
//!
//! 반블록 문자는 한 칸에 두 픽셀밖에 못 담아 흐릿하다. sixel을 아는 터미널(xterm, foot,
//! SIXEL 패치를 넣은 st 등)에서는 이미지를 화면 픽셀 그대로 보낸다.

use image::RgbaImage;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static SUPPORTED: OnceLock<bool> = OnceLock::new();

/// 터미널이 sixel을 아는지 한 번 물어 둔다. 원시 모드(raw mode)에서 불러야 한다.
///
/// `MDVIEW_IMAGES=sixel`이면 묻지 않고 켜고, `cells`면 끈다(반블록).
pub fn detect() {
    SUPPORTED.get_or_init(|| match std::env::var("MDVIEW_IMAGES").ok().as_deref().map(str::trim) {
        Some("sixel") => true,
        Some("cells" | "halfblock" | "off") => false,
        _ => query_da1().is_some_and(|r| da1_has_sixel(&r)),
    });
}

/// sixel을 쓸 수 있으면 글자 한 칸의 픽셀 크기(가로, 세로).
pub fn cell_size() -> Option<(u32, u32)> {
    if !SUPPORTED.get().copied().unwrap_or(false) {
        return None;
    }
    let ws = crossterm::terminal::window_size().ok()?;
    if ws.columns == 0 || ws.rows == 0 || ws.width == 0 || ws.height == 0 {
        return None;
    }
    let (w, h) = (ws.width as u32 / ws.columns as u32, ws.height as u32 / ws.rows as u32);
    (w > 0 && h > 0).then_some((w, h))
}

/// 1차 장치 속성(DA1) 응답에 sixel(4)이 있는지. 예: `ESC [ ? 62 ; 4 c`
fn da1_has_sixel(resp: &str) -> bool {
    let Some(start) = resp.find("\x1b[?") else { return false };
    let body = &resp[start + 3..];
    let body = body.split('c').next().unwrap_or("");
    body.split(';').any(|p| p == "4")
}

/// 터미널에 DA1을 묻고 응답을 읽는다. 답이 없으면 짧게 기다리다 포기한다.
fn query_da1() -> Option<String> {
    use std::os::fd::AsRawFd;
    let mut tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
    tty.write_all(b"\x1b[c").ok()?;
    tty.flush().ok()?;
    let fd = tty.as_raw_fd();
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut buf = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: 유효한 pollfd 하나를 넘긴다.
        let n = unsafe { libc::poll(&mut pfd, 1, left.as_millis() as libc::c_int) };
        if n <= 0 {
            return None;
        }
        let mut chunk = [0u8; 64];
        // SAFETY: chunk 크기만큼만 읽는다.
        let got = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if got <= 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..got as usize]);
        let s = String::from_utf8_lossy(&buf);
        if let Some(i) = s.find("\x1b[?")
            && s[i..].contains('c')
        {
            return Some(s.into_owned());
        }
    }
}

/// 팔레트로 줄인 이미지. `idx`가 `TRANSPARENT`인 픽셀은 칠하지 않는다.
pub struct Indexed {
    pub width: u32,
    pub height: u32,
    palette: Vec<[u8; 3]>,
    idx: Vec<u8>,
}

const TRANSPARENT: u8 = 255;
/// 쓸 수 있는 색 수(투명 표시로 하나를 남긴다)
const MAX_COLORS: usize = 255;

/// 이미지를 최대 255색으로 줄인다(median cut + Floyd–Steinberg 디더링).
pub fn quantize(img: &RgbaImage) -> Indexed {
    let (w, h) = img.dimensions();
    let opaque = |p: &image::Rgba<u8>| p.0[3] >= 128;
    // 팔레트는 표본으로 만든다. 큰 이미지도 몇만 픽셀이면 충분하다.
    let total = (w * h) as usize;
    let step = (total / 60_000).max(1);
    let sample: Vec<[u8; 3]> = img.pixels().step_by(step).filter(|p| opaque(p)).map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
    let palette = median_cut(sample, MAX_COLORS);
    if palette.is_empty() {
        return Indexed { width: w, height: h, palette: vec![[0, 0, 0]], idx: vec![TRANSPARENT; total] };
    }

    // 가까운 색 찾기는 채널당 5비트로 묶어 기억해 둔다.
    let mut memo = vec![u16::MAX; 1 << 15];
    let mut nearest = |c: [i32; 3]| -> u8 {
        let key = ((c[0] as usize >> 3) << 10) | ((c[1] as usize >> 3) << 5) | (c[2] as usize >> 3);
        if memo[key] != u16::MAX {
            return memo[key] as u8;
        }
        let best = palette
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| {
                let d = |i: usize| (p[i] as i32 - c[i]).pow(2);
                2 * d(0) + 4 * d(1) + 3 * d(2)
            })
            .map_or(0, |(i, _)| i as u8);
        memo[key] = best as u16;
        best
    };

    let mut idx = vec![TRANSPARENT; total];
    let wu = w as usize;
    // 이번 줄과 다음 줄의 오차
    let mut err = vec![[0i32; 3]; wu + 2];
    let mut next = vec![[0i32; 3]; wu + 2];
    for y in 0..h as usize {
        for x in 0..wu {
            let p = img.get_pixel(x as u32, y as u32);
            if !opaque(p) {
                continue;
            }
            let want: [i32; 3] = std::array::from_fn(|i| (p.0[i] as i32 + err[x + 1][i] / 16).clamp(0, 255));
            let i = nearest(want);
            idx[y * wu + x] = i;
            let got = palette[i as usize];
            for c in 0..3 {
                let e = want[c] - got[c] as i32;
                err[x + 2][c] += e * 7;
                next[x][c] += e * 3;
                next[x + 1][c] += e * 5;
                next[x + 2][c] += e;
            }
        }
        std::mem::swap(&mut err, &mut next);
        next.iter_mut().for_each(|e| *e = [0; 3]);
    }
    Indexed { width: w, height: h, palette, idx }
}

/// 색 상자를 가장 넓은 채널의 중앙값에서 계속 나눠 `n`개 이하의 대표색을 만든다.
fn median_cut(pixels: Vec<[u8; 3]>, n: usize) -> Vec<[u8; 3]> {
    if pixels.is_empty() {
        return Vec::new();
    }
    let range = |b: &[[u8; 3]]| -> (usize, u8) {
        (0..3)
            .map(|c| {
                let (lo, hi) = b.iter().fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
                (c, hi - lo)
            })
            .max_by_key(|&(_, r)| r)
            .unwrap()
    };
    let mut boxes = vec![pixels];
    while boxes.len() < n {
        // 넓고 붐비는 상자부터 나눈다.
        let Some((bi, ch)) = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| (i, range(b)))
            .filter(|(i, (_, r))| *r > 0 && boxes[*i].len() > 1)
            .max_by_key(|(i, (_, r))| *r as usize * (boxes[*i].len() as f64).sqrt() as usize)
            .map(|(i, (c, _))| (i, c))
        else {
            break;
        };
        let mut b = boxes.swap_remove(bi);
        b.sort_unstable_by_key(|p| p[ch]);
        // 같은 값이 양쪽 상자로 갈라지지 않게 값 경계에서 자른다.
        let v = b[b.len() / 2][ch];
        let at = match b.partition_point(|p| p[ch] < v) {
            0 => b.partition_point(|p| p[ch] <= v),
            i => i,
        };
        let hi = b.split_off(at);
        boxes.push(b);
        boxes.push(hi);
    }
    boxes
        .iter()
        .map(|b| {
            let s = b.iter().fold([0u64; 3], |mut s, p| {
                (0..3).for_each(|c| s[c] += p[c] as u64);
                s
            });
            std::array::from_fn(|c| (s[c] / b.len() as u64) as u8)
        })
        .collect()
}

/// `img`의 `y0`부터 `h` 줄을 sixel 문자열로 만든다. 투명 픽셀은 배경을 그대로 둔다.
pub fn encode(img: &Indexed, y0: u32, h: u32) -> String {
    let w = img.width as usize;
    let h = h.min(img.height.saturating_sub(y0)) as usize;
    let mut out = String::with_capacity(w * h / 2);
    // P2=1: 칠하지 않은 픽셀은 투명
    out.push_str(&format!("\x1bP0;1;0q\"1;1;{w};{h}"));
    for (i, c) in img.palette.iter().enumerate() {
        let pct = |v: u8| (v as u32 * 100 + 127) / 255;
        out.push_str(&format!("#{i};2;{};{};{}", pct(c[0]), pct(c[1]), pct(c[2])));
    }
    let mut bits: Vec<Vec<u8>> = vec![Vec::new(); img.palette.len()];
    let mut used: Vec<usize> = Vec::new();
    for band in (0..h).step_by(6) {
        for r in 0..6.min(h - band) {
            let row = (y0 as usize + band + r) * w;
            for x in 0..w {
                let c = img.idx[row + x];
                if c == TRANSPARENT {
                    continue;
                }
                let v = &mut bits[c as usize];
                if v.is_empty() {
                    v.resize(w, 0);
                    used.push(c as usize);
                }
                v[x] |= 1 << r;
            }
        }
        for (k, &c) in used.iter().enumerate() {
            if k > 0 {
                out.push('$');
            }
            out.push_str(&format!("#{c}"));
            let v = &bits[c];
            // 끝의 빈 칸은 쓰지 않는다.
            let end = v.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
            let mut x = 0;
            while x < end {
                let b = v[x];
                let run = v[x..end].iter().take_while(|&&o| o == b).count();
                let ch = (63 + b) as char;
                if run > 3 {
                    out.push_str(&format!("!{run}{ch}"));
                } else {
                    (0..run).for_each(|_| out.push(ch));
                }
                x += run;
            }
        }
        for &c in &used {
            bits[c].clear();
        }
        used.clear();
        out.push('-');
    }
    out.push_str("\x1b\\");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da1_parsing() {
        assert!(da1_has_sixel("\x1b[?62;4c"));
        assert!(da1_has_sixel("\x1b[?64;1;4;6;22c"));
        assert!(!da1_has_sixel("\x1b[?6c"));
        assert!(!da1_has_sixel("\x1b[?62;44c"));
    }

    #[test]
    fn quantize_keeps_few_colors_exact() {
        let mut img = RgbaImage::new(4, 2);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) };
        }
        img.put_pixel(3, 1, image::Rgba([0, 0, 0, 0]));
        let q = quantize(&img);
        assert_eq!(q.palette.len(), 2);
        assert_eq!(q.palette[q.idx[0] as usize], [255, 0, 0]);
        assert_eq!(q.palette[q.idx[2] as usize], [0, 0, 255]);
        assert_eq!(q.idx[7], TRANSPARENT);
    }

    #[test]
    fn encodes_bands_with_run_length() {
        let img = Indexed { width: 5, height: 2, palette: vec![[255, 255, 255]], idx: vec![0; 10] };
        // 두 줄이 칠해졌으니 비트 0b11 → '?'+3 = 'B', 다섯 번 반복
        assert_eq!(encode(&img, 0, 2), "\x1bP0;1;0q\"1;1;5;2#0;2;100;100;100#0!5B-\x1b\\");
        // 둘째 줄만 잘라내면 비트 0b1 → '@'
        assert_eq!(encode(&img, 1, 1), "\x1bP0;1;0q\"1;1;5;1#0;2;100;100;100#0!5@-\x1b\\");
    }
}

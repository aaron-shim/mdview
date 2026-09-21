//! 문서 위에 sixel 이미지를 얹는 층. 페이저와 파일 브라우저 미리보기가 함께 쓴다.

use crate::render::Placement;
use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use std::io::Write;
use std::sync::Arc;

/// 화면에 그린 이미지 한 장(또는 스크롤로 잘린 일부).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    /// 이미지 데이터의 주소. 다시 렌더링해 바뀌었는지 알아보는 데 쓴다.
    image: usize,
    x: u16,
    y: u16,
    /// 이미지 안에서 잘라 그리는 픽셀 줄 범위
    y0: u32,
    h: u32,
}

/// 지금 화면에 그려 둔 이미지들을 기억한다.
#[derive(Default)]
pub struct ImageLayer {
    shown: Vec<Shown>,
}

impl ImageLayer {
    /// 화면의 이미지를 `area`에 `scroll` 줄부터 보이는 문서에 맞춘다.
    /// 달라졌으면 화면을 지우고 `redraw`로 글자를 다시 그린 뒤 sixel을 보낸다.
    ///
    /// 터미널은 글자를 덮어쓴 칸의 이미지만 지우는데, ratatui는 바뀐 칸만 다시 쓰므로
    /// 옮겨 간 이미지의 흔적이 남는다. 그래서 이미지가 움직이면 화면 전체를 새로 그린다.
    pub fn sync(
        &mut self,
        terminal: &mut DefaultTerminal,
        images: &[Placement],
        area: Rect,
        scroll: usize,
        redraw: impl FnOnce(&mut DefaultTerminal) -> Result<()>,
    ) -> Result<()> {
        let want = visible(images, area, scroll);
        if want.iter().map(|(_, s)| s).eq(self.shown.iter()) {
            return Ok(());
        }
        if !self.shown.is_empty() {
            terminal.clear()?;
            redraw(terminal)?;
        }
        let mut out = std::io::stdout().lock();
        for (i, s) in &want {
            let data = crate::sixel::encode(&images[*i].image, s.y0, s.h);
            crossterm::queue!(out, crossterm::cursor::MoveTo(s.x, s.y))?;
            out.write_all(data.as_bytes())?;
        }
        out.flush()?;
        self.shown = want.into_iter().map(|(_, s)| s).collect();
        Ok(())
    }

    /// 화면에 남은 이미지를 지운다(다른 화면으로 넘어가기 전).
    pub fn clear(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        if !self.shown.is_empty() {
            terminal.clear()?;
            self.shown.clear();
        }
        Ok(())
    }

    /// 화면이 다른 이유로 지워졌을 때(크기 변경, 다른 화면에서 복귀) 다음에 다시 보내도록 잊는다.
    pub fn forget(&mut self) {
        self.shown.clear();
    }
}

/// `area`에 `scroll` 줄부터 보일 때 화면에 걸치는 이미지들과 잘라 그릴 범위.
fn visible(images: &[Placement], area: Rect, scroll: usize) -> Vec<(usize, Shown)> {
    let page = area.height as usize;
    let mut out = Vec::new();
    for (i, p) in images.iter().enumerate() {
        let start = p.line.max(scroll);
        let end = (p.line + p.rows).min(scroll + page);
        if start >= end || p.col >= area.width as usize {
            continue;
        }
        let y0 = (start - p.line) as u32 * p.cell_h;
        if y0 >= p.image.height {
            continue;
        }
        let room = (end - start) as u32 * p.cell_h;
        let mut h = (p.image.height - y0).min(room);
        // sixel은 6픽셀 단위로 그려진다. 올림한 높이가 자리를 넘으면 아래 줄을 덮지 않게 내림한다.
        if h.div_ceil(6) * 6 > room {
            h = room / 6 * 6;
        }
        if h == 0 {
            continue;
        }
        let shown = Shown {
            image: Arc::as_ptr(&p.image) as usize,
            x: area.x + p.col as u16,
            y: area.y + (start - scroll) as u16,
            y0,
            h,
        };
        out.push((i, shown));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sixel::quantize;

    fn placement(line: usize, rows: usize, h: u32) -> Placement {
        let img = image::RgbaImage::from_pixel(4, h, image::Rgba([1, 2, 3, 255]));
        Placement { line, col: 2, rows, cell_h: 10, image: Arc::new(quantize(&img)) }
    }

    #[test]
    fn crops_images_cut_by_the_viewport() {
        let area = Rect::new(5, 1, 80, 20);
        // 10~14줄에 걸친 50픽셀 높이 이미지
        let imgs = [placement(10, 5, 50)];
        // 전부 보일 때: 50은 6의 배수로 올리면 54라 자리(50)를 넘으므로 48로 내린다
        let v = visible(&imgs, area, 0);
        assert_eq!((v[0].1.x, v[0].1.y, v[0].1.y0, v[0].1.h), (7, 11, 0, 48));
        // 위 두 줄이 가려지면 20픽셀부터 30픽셀
        let v = visible(&imgs, area, 12);
        assert_eq!((v[0].1.y, v[0].1.y0, v[0].1.h), (1, 20, 30));
        // 아래가 잘리면 보이는 두 줄(20픽셀) 안에서 18픽셀
        let v = visible(&imgs, Rect::new(5, 1, 80, 12), 0);
        assert_eq!((v[0].1.y0, v[0].1.h), (0, 18));
        // 화면 밖이면 없다
        assert!(visible(&imgs, area, 15).is_empty());
        assert!(visible(&imgs, Rect::new(0, 0, 80, 5), 0).is_empty());
    }
}

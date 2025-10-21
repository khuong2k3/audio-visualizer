use std::io::{self, Stdout, Write};

use crossterm::{cursor, style::{self, StyledContent, Stylize}, QueueableCommand};

pub struct Buffer<D> {
    bufs: [Vec<StyledContent<char>>; 2],
    current: usize,
    width: usize,
    height: usize,
    on_update: Option<Box<dyn FnMut(&mut [StyledContent<char>], usize, usize, D)>>,
    resized: bool,
}

#[allow(unused)]
impl<D> Buffer<D> {
    pub fn new() -> Self {
        Self {
            bufs: [Vec::new(), Vec::new()],
            current: 0,
            width: 0,
            height: 0,
            on_update: None,
            resized: false,
        }
    }

    pub fn on_update(&mut self, on_update: impl FnMut(&mut [StyledContent<char>], usize, usize, D) + 'static) {
        self.on_update = Some(Box::new(on_update));
    }

    fn get<'a>(&'a self) -> &'a Vec<StyledContent<char>> {
        &self.bufs[self.current]
    }

    fn next(&mut self) {
        self.current = (self.current + 1) % 2;
    }

    fn diff(&self, x: usize, y: usize) -> bool {
        let idx = y * self.width + x;

        self.bufs[0][idx] != self.bufs[1][idx]
    }

    fn check_resized(&self, new_w: usize, new_h: usize) -> bool {
        self.width != new_w || self.height != new_h
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn resize(&mut self, new_w: usize, new_h: usize) {
        if !self.check_resized(new_w, new_h) {
            return;
        }
        self.resized = true;

        let new_size = new_w * new_h;
        if new_size != self.width * self.height {
            self.bufs = [vec![' '.stylize(); new_size], vec![' '.stylize(); new_size]];
        }
        self.width = new_w;
        self.height = new_h;
    }

    pub fn update(&mut self, update_data: D) {
        if let Some(on_update) = &mut self.on_update {
            let new_idx = (self.current + 1) % 2;

            on_update(&mut self.bufs[new_idx], self.width, self.height, update_data);
        }
    }

    pub fn present(&mut self, stdout: &mut Stdout) -> io::Result<()> {
        let content = self.get();

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = y * self.width + x;

                if self.resized || self.diff(x, y) {
                    stdout.queue(cursor::MoveTo(x as u16, y as u16))?
                        .queue(style::PrintStyledContent(content[idx]))?;
                }
            }
        }
        self.resized = false;
        self.next();

        stdout.flush()?;

        Ok(())
    }
}


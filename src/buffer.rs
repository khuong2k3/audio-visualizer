use std::io::{self, Stdout, Write};

use crossterm::{cursor, style::{self, StyledContent, Stylize}, QueueableCommand};

pub struct Buffer {
    bufs: [Vec<StyledContent<char>>; 2],
    current: usize,
    width: usize,
    height: usize,
    on_update: Option<Box<dyn FnMut(&mut [StyledContent<char>], usize, usize)>> 
}

impl Buffer {
    fn new() -> Self {
        Self {
            bufs: [Vec::new(), Vec::new()],
            current: 0,
            width: 0,
            height: 0,
            on_update: None,
        }
    }

    fn on_update(&mut self, on_update: impl FnMut(&mut [StyledContent<char>], usize, usize) + 'static) {
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

    pub fn resize(&mut self, new_w: usize, new_h: usize) {
        if self.width < new_w {
            self.width = new_w;
        }

        if self.height < new_h {
            self.height = new_h;
        }

        if self.width >= new_w || self.height >= new_h {
            let size = new_w * new_h;
            self.width = new_w;
            self.height = new_h;

            self.bufs = [vec![' '.stylize(); size], vec![' '.stylize(); size]];
        }
    }

    fn update(&mut self) {
        if let Some(on_update) = &mut self.on_update {
            let new_idx = (self.current + 1) % 2;

            on_update(&mut self.bufs[new_idx], self.width, self.height);
            self.current = new_idx;
        }
    }

    pub fn present(&self, stdout: &mut Stdout, force: bool) -> io::Result<()> {
        let content = self.get();

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = y * self.width + x;

                if force || self.diff(x, y) {
                    stdout.queue(cursor::MoveTo(x as u16, y as u16))?
                        .queue(style::PrintStyledContent(content[idx]))?;
                }
            }
        }

        stdout.flush()?;

        Ok(())
    }
}


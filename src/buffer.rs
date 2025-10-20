use std::io::{self, Stdout, Write};

use crossterm::{cursor, style::{self, StyledContent, Stylize}, QueueableCommand};

pub struct Buffer<D> {
    bufs: [Vec<StyledContent<char>>; 2],
    current: usize,
    width: usize,
    height: usize,
    on_update: Option<Box<dyn FnMut(&mut [StyledContent<char>], usize, usize, D)>> 
}

impl<D> Buffer<D> {
    pub fn new() -> Self {
        Self {
            bufs: [Vec::new(), Vec::new()],
            current: 0,
            width: 0,
            height: 0,
            on_update: None,
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

    pub fn resize(&mut self, new_w: usize, new_h: usize) {
        if !self.check_resized(new_w, new_h) {
            return;
        }

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

    pub fn update(&mut self, update_data: D) {
        if let Some(on_update) = &mut self.on_update {
            let new_idx = (self.current + 1) % 2;

            on_update(&mut self.bufs[new_idx], self.width, self.height, update_data);
        }
    }

    pub fn present(&mut self, stdout: &mut Stdout, force: bool) -> io::Result<()> {
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
        self.next();

        stdout.flush()?;

        Ok(())
    }
}


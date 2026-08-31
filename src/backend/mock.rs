//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Explicit opt-in console backend used by tests and development.

use super::{BackendError, ConsoleFrame, ConsoleSession};
use image::{ImageBuffer, Rgba, RgbaImage};
use serde_json::Value;

pub struct MockBackend {
    vmid: u32,
    width: u32,
    height: u32,
    frame_number: u8,
}

impl MockBackend {
    pub fn open(vmid: u32) -> Result<Self, BackendError> {
        Ok(Self {
            vmid,
            width: 640,
            height: 480,
            frame_number: 0,
        })
    }
}

impl ConsoleSession for MockBackend {
    fn backend_kind(&self) -> &'static str {
        "mock"
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn frame(&mut self) -> Result<ConsoleFrame, BackendError> {
        self.frame_number = self.frame_number.wrapping_add(1);
        let colour = [(self.vmid & 0xff) as u8, self.frame_number, 0x80, 0xff];
        let image: RgbaImage = ImageBuffer::from_pixel(self.width, self.height, Rgba(colour));
        let mut output = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        Ok(ConsoleFrame {
            width: self.width,
            height: self.height,
            format: "png".into(),
            bytes: output.into_inner(),
        })
    }

    fn input(&mut self, events: &[Value]) -> Result<(), BackendError> {
        tracing::debug!(
            vmid = self.vmid,
            event_count = events.len(),
            "mock console input"
        );
        Ok(())
    }

    fn close(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MockBackend;
    use crate::backend::ConsoleSession;

    #[test]
    fn frame_is_a_non_empty_png() {
        let mut backend = MockBackend::open(100).unwrap();
        let frame = backend.frame().unwrap();
        assert!(frame.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(frame.bytes.len() > 100);
    }
}

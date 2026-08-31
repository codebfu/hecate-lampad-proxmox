//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal synchronous RFB 3.8 client with Raw framebuffer support.

use super::{BackendError, ConsoleFrame, ConsoleSession};
use des::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
use des::Des;
use image::{ImageBuffer, RgbaImage};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct RfbSession {
    stream: TcpStream,
    width: u16,
    height: u16,
    rgba: Vec<u8>,
    kind: &'static str,
}

impl RfbSession {
    pub fn connect(
        host: &str,
        port: u16,
        password: Option<&str>,
        kind: &'static str,
    ) -> Result<Self, BackendError> {
        let mut stream = TcpStream::connect((host, port))?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        stream.set_write_timeout(Some(Duration::from_secs(15)))?;

        let mut version = [0u8; 12];
        stream.read_exact(&mut version)?;
        if !version.starts_with(b"RFB 003.") {
            return Err(BackendError::Protocol("invalid RFB server version".into()));
        }
        stream.write_all(b"RFB 003.008\n")?;

        let count = read_u8(&mut stream)? as usize;
        if count == 0 {
            return Err(BackendError::Protocol(read_reason(&mut stream)?));
        }
        let mut security = vec![0u8; count];
        stream.read_exact(&mut security)?;
        let selected = if password.is_some() && security.contains(&2) {
            2
        } else if security.contains(&1) {
            1
        } else if security.contains(&2) {
            2
        } else {
            return Err(BackendError::Protocol(format!(
                "unsupported RFB security types: {security:?}"
            )));
        };
        stream.write_all(&[selected])?;
        if selected == 2 {
            let password = password.ok_or_else(|| {
                BackendError::Protocol("VNC authentication requires a password".into())
            })?;
            let mut challenge = [0u8; 16];
            stream.read_exact(&mut challenge)?;
            stream.write_all(&vnc_auth_response(password, challenge))?;
        }
        let status = read_u32(&mut stream)?;
        if status != 0 {
            let reason = read_reason(&mut stream)
                .unwrap_or_else(|_| format!("RFB authentication failed ({status})"));
            return Err(BackendError::Protocol(reason));
        }

        stream.write_all(&[1])?;
        let width = read_u16(&mut stream)?;
        let height = read_u16(&mut stream)?;
        let mut server_format_and_name_len = [0u8; 20];
        stream.read_exact(&mut server_format_and_name_len)?;
        let name_len = u32::from_be_bytes(
            server_format_and_name_len[16..20]
                .try_into()
                .expect("fixed slice"),
        ) as usize;
        let mut name = vec![0u8; name_len];
        stream.read_exact(&mut name)?;

        // 32 bpp, little-endian, true colour, RGB max 255, shifts 16/8/0.
        let pixel_format = [
            0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0,
        ];
        stream.write_all(&pixel_format)?;
        // Raw encoding only.
        stream.write_all(&[2, 0, 0, 1, 0, 0, 0, 0])?;

        Ok(Self {
            stream,
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
            kind,
        })
    }

    fn request_update(&mut self) -> Result<(), BackendError> {
        let mut request = vec![3, 0, 0, 0, 0, 0];
        request.extend_from_slice(&self.width.to_be_bytes());
        request.extend_from_slice(&self.height.to_be_bytes());
        self.stream.write_all(&request)?;

        loop {
            match read_u8(&mut self.stream)? {
                0 => break,
                2 => continue,
                3 => {
                    let mut padding = [0u8; 3];
                    self.stream.read_exact(&mut padding)?;
                    let length = read_u32(&mut self.stream)? as usize;
                    let mut text = vec![0u8; length];
                    self.stream.read_exact(&mut text)?;
                }
                message => {
                    return Err(BackendError::Protocol(format!(
                        "unsupported RFB server message {message}"
                    )))
                }
            }
        }

        let mut padding = [0u8; 1];
        self.stream.read_exact(&mut padding)?;
        let rectangles = read_u16(&mut self.stream)?;
        for _ in 0..rectangles {
            let x = read_u16(&mut self.stream)?;
            let y = read_u16(&mut self.stream)?;
            let width = read_u16(&mut self.stream)?;
            let height = read_u16(&mut self.stream)?;
            let encoding = read_i32(&mut self.stream)?;
            if encoding == -223 {
                self.width = width;
                self.height = height;
                self.rgba.resize(width as usize * height as usize * 4, 0);
                continue;
            }
            if encoding != 0 {
                return Err(BackendError::Protocol(format!(
                    "server used unsupported encoding {encoding}"
                )));
            }
            let mut raw = vec![0u8; width as usize * height as usize * 4];
            self.stream.read_exact(&mut raw)?;
            for row in 0..height as usize {
                for column in 0..width as usize {
                    let source = (row * width as usize + column) * 4;
                    let target =
                        (((y as usize + row) * self.width as usize) + x as usize + column) * 4;
                    if target + 3 < self.rgba.len() {
                        self.rgba[target] = raw[source + 2];
                        self.rgba[target + 1] = raw[source + 1];
                        self.rgba[target + 2] = raw[source];
                        self.rgba[target + 3] = 255;
                    }
                }
            }
        }
        Ok(())
    }
}

impl ConsoleSession for RfbSession {
    fn backend_kind(&self) -> &'static str {
        self.kind
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width.into(), self.height.into())
    }

    fn frame(&mut self) -> Result<ConsoleFrame, BackendError> {
        self.request_update()?;
        let image: RgbaImage =
            ImageBuffer::from_raw(self.width.into(), self.height.into(), self.rgba.clone())
                .ok_or_else(|| BackendError::Protocol("invalid framebuffer dimensions".into()))?;
        let mut output = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        Ok(ConsoleFrame {
            width: self.width.into(),
            height: self.height.into(),
            format: "png".into(),
            bytes: output.into_inner(),
        })
    }

    fn input(&mut self, events: &[Value]) -> Result<(), BackendError> {
        for event in events {
            match event.get("type").and_then(Value::as_str) {
                Some("key") => {
                    let down = event.get("down").and_then(Value::as_bool).unwrap_or(true);
                    let key = event.get("key").and_then(Value::as_u64).ok_or_else(|| {
                        BackendError::Invalid("key event requires numeric key".into())
                    })? as u32;
                    let mut message = vec![4, u8::from(down), 0, 0];
                    message.extend_from_slice(&key.to_be_bytes());
                    self.stream.write_all(&message)?;
                }
                Some("pointer") => {
                    let buttons = event.get("buttons").and_then(Value::as_u64).unwrap_or(0) as u8;
                    let x = event.get("x").and_then(Value::as_u64).unwrap_or(0) as u16;
                    let y = event.get("y").and_then(Value::as_u64).unwrap_or(0) as u16;
                    let mut message = vec![5, buttons];
                    message.extend_from_slice(&x.to_be_bytes());
                    message.extend_from_slice(&y.to_be_bytes());
                    self.stream.write_all(&message)?;
                }
                other => {
                    return Err(BackendError::Invalid(format!(
                        "unsupported input event type: {other:?}"
                    )))
                }
            }
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), BackendError> {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
}

fn vnc_auth_response(password: &str, challenge: [u8; 16]) -> [u8; 16] {
    let mut key = [0u8; 8];
    for (target, source) in key.iter_mut().zip(password.as_bytes().iter().copied()) {
        *target = source.reverse_bits();
    }
    let cipher = Des::new(GenericArray::from_slice(&key));
    let mut response = challenge;
    cipher.encrypt_block(GenericArray::from_mut_slice(&mut response[..8]));
    cipher.encrypt_block(GenericArray::from_mut_slice(&mut response[8..]));
    response
}

fn read_u8(stream: &mut TcpStream) -> Result<u8, BackendError> {
    let mut value = [0u8; 1];
    stream.read_exact(&mut value)?;
    Ok(value[0])
}

fn read_u16(stream: &mut TcpStream) -> Result<u16, BackendError> {
    let mut value = [0u8; 2];
    stream.read_exact(&mut value)?;
    Ok(u16::from_be_bytes(value))
}

fn read_u32(stream: &mut TcpStream) -> Result<u32, BackendError> {
    let mut value = [0u8; 4];
    stream.read_exact(&mut value)?;
    Ok(u32::from_be_bytes(value))
}

fn read_i32(stream: &mut TcpStream) -> Result<i32, BackendError> {
    let mut value = [0u8; 4];
    stream.read_exact(&mut value)?;
    Ok(i32::from_be_bytes(value))
}

fn read_reason(stream: &mut TcpStream) -> Result<String, BackendError> {
    let length = read_u32(stream)? as usize;
    let mut reason = vec![0u8; length];
    stream.read_exact(&mut reason)?;
    Ok(String::from_utf8_lossy(&reason).into_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn vnc_auth_is_deterministic() {
        let challenge = [0x11; 16];
        assert_eq!(
            super::vnc_auth_response("password", challenge),
            super::vnc_auth_response("password", challenge)
        );
    }
}

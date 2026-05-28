use serde_json::Value;
use std::io::{self, BufRead, Write};

pub struct JsonRpcStream<R, W> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> JsonRpcStream<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub fn read_message(&mut self) -> io::Result<Option<Value>> {
        let mut size = None;
        let mut buffer = String::new();

        loop {
            buffer.clear();
            let n = self.reader.read_line(&mut buffer)?;
            if n == 0 {
                return Ok(None); // EOF
            }

            let line = buffer.trim();
            if line.is_empty() {
                break; // End of headers
            }

            if line.to_lowercase().starts_with("content-length:") {
                if let Some(rest) = line.split(':').nth(1) {
                    match rest.trim().parse::<usize>() {
                        Ok(s) => size = Some(s),
                        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid Content-Length: {}", e))),
                    }
                }
            }
        }

        let size = match size {
            Some(s) => s,
            None => return Ok(None), // No Content-Length found or empty headers
        };

        let mut body = vec![0; size];
        self.reader.read_exact(&mut body)?;

        let text = String::from_utf8(body).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
        })?;

        let message: Value = serde_json::from_str(&text).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid JSON: {}", e))
        })?;

        Ok(Some(message))
    }

    pub fn send_message(&mut self, message: &Value) -> io::Result<()> {
        let json = serde_json::to_string(message)?;
        let len = json.len();
        write!(self.writer, "Content-Length: {}\r\n\r\n{}", len, json)?;
        self.writer.flush()?;
        Ok(())
    }
}

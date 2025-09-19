///
/// win_can.rs
///
/// Implementation of CanInterface for Windows using pipes.
/// Will require an existing pipe server to be connected to a CAN port using the 'win_can_utils' package.
///
use crate::{CanInterface, can::CanFrame};
use std::io::{Error as IoError, ErrorKind};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

pub struct WindowsCan {
    reader: Option<BufReader<NamedPipeClient>>,
    writer: Option<NamedPipeClient>,
}

impl CanInterface for WindowsCan {
    /// Open a CAN device
    ///
    /// Can device is usually attached to a serial COM port (i.e. COM5). This method will open two separate pipes for reading and writing.
    fn open(channel: &str) -> tokio::io::Result<Self> {
        let sanitized = channel
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();
        let out_pipe_name = format!(r"\\.\pipe\can_{}_out", sanitized);
        let in_pipe_name = format!(r"\\.\pipe\can_{}_in", sanitized);

        let out_pipe = ClientOptions::new().open(&out_pipe_name)?;
        let in_pipe = ClientOptions::new().open(&in_pipe_name)?;

        Ok(Self {
            reader: Some(BufReader::new(out_pipe)),
            writer: Some(in_pipe),
        })
    }

    async fn read_frame(&mut self) -> tokio::io::Result<CanFrame> {
        let reader = match &mut self.reader {
            Some(r) => r,
            None => {
                return Err(IoError::new(
                    ErrorKind::InvalidData,
                    "No read pipe has been opened",
                ));
            }
        };

        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err(IoError::new(ErrorKind::UnexpectedEof, "Pipe closed"));
        }

        let json: serde_json::Value = serde_json::from_str(&line.trim())
            .map_err(|e| IoError::new(ErrorKind::InvalidData, e))?;

        let id = json["id"]
            .as_u64()
            .ok_or_else(|| IoError::new(ErrorKind::InvalidData, "Missing id"))?
            as u32;
        let extended = json["is_extended"].as_bool().unwrap_or(false);
        let rtr = json["rtr"].as_bool().unwrap_or(false);
        let err = json["error"].as_bool().unwrap_or(false);

        let data = json["data"]
            .as_array()
            .ok_or_else(|| IoError::new(ErrorKind::InvalidData, "Missing data"))?
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u8)
            .collect::<Vec<_>>();

        let mut frame = if rtr {
            CanFrame::new_remote(id, data.len(), extended)
        } else if err {
            CanFrame::new_error(id)
        } else if extended {
            CanFrame::new_eff(id, &data)
        } else {
            CanFrame::new(id, &data)
        }
        .map_err(|e| IoError::new(ErrorKind::InvalidData, e))?;

        frame.set_timestamp(json.get("timestamp").and_then(|v| v.as_u64()));

        Ok(frame)
    }

    async fn write_frame(&mut self, frame: CanFrame) -> tokio::io::Result<()> {
        let writer = match &mut self.writer {
            Some(r) => r,
            None => {
                return Err(IoError::new(
                    ErrorKind::InvalidData,
                    "No write pipe has been opened",
                ));
            }
        };

        writer.write_all(bytemuck::bytes_of(&frame)).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    }
}

impl WindowsCan {
    /// Open a read-only CAN device
    ///
    /// Can device is usually attached to a serial COM port (i.e. COM5). This method will a single pipe for reading CAN messages. Attempting to write to the port later will throw an InvalidData error.
    pub fn open_read_only(channel: &str) -> tokio::io::Result<Self> {
        let sanitized = channel
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();
        let out_pipe_name = format!(r"\\.\pipe\can_{}_out", sanitized);

        let out_pipe = ClientOptions::new().open(&out_pipe_name)?;

        Ok(Self {
            reader: Some(BufReader::new(out_pipe)),
            writer: None,
        })
    }

    /// Open a write-only CAN device
    ///
    /// Can device is usually attached to a serial COM port (i.e. COM5). This method will a single pipe for writing CAN messages. Attempting to read from the port later will throw an InvalidData error.
    pub fn open_write_only(channel: &str) -> tokio::io::Result<Self> {
        let sanitized = channel
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();
        let in_pipe_name = format!(r"\\.\pipe\can_{}_in", sanitized);

        let in_pipe = ClientOptions::new().open(&in_pipe_name)?;

        Ok(Self {
            reader: None,
            writer: Some(in_pipe),
        })
    }
}

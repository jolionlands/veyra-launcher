//! Synchronous named-pipe client for the Aurora wallpaper engine.
//!
//! Aurora exposes a JSON-over-named-pipe IPC at `\\.\pipe\aurora-{session_id}` —
//! the pipe name is scoped to the current Windows session, exactly like the
//! daemon's own client (`pipe_path_for_session` in
//! `Development/tools/WM/aurora/src/ipc/mod.rs`). Each message is one
//! length-prefixed frame: a u32-LE byte count followed by the JSON payload.
//! The daemon handles one message per connection: it answers with exactly one
//! framed response and then shuts the pipe down.
//!
//! This module mirrors the daemon's client (`open_pipe_client`, `write_frame`,
//! `read_frame`) 1:1, adapted to veyra's synchronous Win32 API style.

use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, GENERIC_READ,
    GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers,
    OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;

use crate::PlatformError;

/// Mirror the daemon's `MAX_FRAME_SIZE`: JSON payloads are capped at 1 MiB.
const MAX_FRAME_SIZE: usize = 1024 * 1024;
/// Mirror the daemon's `FRAME_IO_TIMEOUT`: bound connect and frame I/O.
const FRAME_IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Retry cadence while the daemon reports `ERROR_PIPE_BUSY` (mirror daemon).
const PIPE_BUSY_RETRY_STEP: Duration = Duration::from_millis(50);
/// Legacy bare pipe name, kept as a one-shot fallback for older daemon builds;
/// the daemon (v0.1.0) only serves the session-scoped name.
const AURORA_PIPE_PATH_FALLBACK: &str = r"\\.\pipe\aurora";

/// Current Windows session id. Both veyra and the daemon run as user processes
/// in the same interactive session, so this matches the daemon's
/// `current_session_id()`.
fn current_session_id() -> Result<u32, PlatformError> {
    let mut session_id: u32 = 0;
    let ok = unsafe { ProcessIdToSessionId(std::process::id(), &mut session_id) };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        return Err(PlatformError::AuroraIpcFailed {
            message: format!("ProcessIdToSessionId failed (error {code})"),
        });
    }
    Ok(session_id)
}

/// Session-scoped pipe name, byte-for-byte as the daemon computes it.
fn pipe_path_for_session(session_id: u32) -> String {
    format!(r"\\.\pipe\aurora-{session_id}")
}

/// Open the daemon pipe with the daemon client's retry semantics: retry
/// `ERROR_PIPE_BUSY` every 50 ms until the 5 s deadline, and try the legacy
/// bare pipe name once on `ERROR_FILE_NOT_FOUND` before failing.
fn open_pipe(mut path: &str) -> Result<HANDLE, PlatformError> {
    let deadline = Instant::now() + FRAME_IO_TIMEOUT;
    let mut tried_fallback = false;
    loop {
        let wide_path: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return Ok(handle);
        }
        let code = unsafe { GetLastError() };
        if code == ERROR_PIPE_BUSY {
            if Instant::now() >= deadline {
                return Err(PlatformError::AuroraIpcFailed {
                    message: format!(
                        "Cannot connect to aurora daemon at {path}: IPC remained busy for {} ms",
                        FRAME_IO_TIMEOUT.as_millis()
                    ),
                });
            }
            std::thread::sleep(PIPE_BUSY_RETRY_STEP);
            continue;
        }
        if code == ERROR_FILE_NOT_FOUND && !tried_fallback {
            tried_fallback = true;
            path = AURORA_PIPE_PATH_FALLBACK;
            continue;
        }
        return Err(PlatformError::AuroraIpcFailed {
            message: if code == ERROR_FILE_NOT_FOUND {
                format!(
                    "Cannot connect to aurora daemon at {path}: aurora is not running in this Windows session (error {code})"
                )
            } else {
                format!("Cannot connect to aurora daemon at {path}: error {code}")
            },
        });
    }
}

/// Minimal byte-stream abstraction so the framing logic is unit-testable in
/// memory. `read_some` returns `Ok(None)` on a clean end of stream (pipe
/// closed), `Ok(Some(n))` with `n > 0` on progress — byte-mode named pipes
/// return short reads, so callers must loop.
trait PipeIo {
    fn write_all_bytes(&mut self, buf: &[u8]) -> Result<(), PlatformError>;
    fn read_some(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PlatformError>;
}

impl PipeIo for HANDLE {
    fn write_all_bytes(&mut self, mut buf: &[u8]) -> Result<(), PlatformError> {
        while !buf.is_empty() {
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    *self,
                    buf.as_ptr(),
                    buf.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let code = unsafe { GetLastError() };
                return Err(PlatformError::AuroraIpcFailed {
                    message: format!("Failed to write Aurora IPC request (error {code})"),
                });
            }
            if written == 0 {
                return Err(PlatformError::AuroraIpcFailed {
                    message: "Aurora IPC write made no progress".to_string(),
                });
            }
            buf = &buf[written as usize..];
        }
        Ok(())
    }

    fn read_some(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PlatformError> {
        if buf.is_empty() {
            return Ok(Some(0));
        }
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                *self,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            // The daemon closes the pipe after sending its response; treat
            // that as a clean end of stream.
            if code == ERROR_BROKEN_PIPE {
                return Ok(None);
            }
            return Err(PlatformError::AuroraIpcFailed {
                message: format!("Failed to read Aurora IPC response (error {code})"),
            });
        }
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(read as usize))
    }
}

/// Write one length-prefixed frame: u32-LE payload length + payload bytes,
/// mirroring the daemon's `write_frame`.
fn write_frame<I: PipeIo>(io: &mut I, payload: &[u8]) -> Result<(), PlatformError> {
    if payload.len() > MAX_FRAME_SIZE {
        return Err(PlatformError::AuroraIpcFailed {
            message: format!("Aurora IPC frame exceeds {MAX_FRAME_SIZE} byte limit"),
        });
    }
    io.write_all_bytes(&(payload.len() as u32).to_le_bytes())?;
    io.write_all_bytes(payload)?;
    Ok(())
}

/// Read exactly one length-prefixed frame. A clean end of stream before any
/// header byte is `Ok(None)`; truncated headers or payloads are errors —
/// mirroring the daemon's `read_frame`.
fn read_frame<I: PipeIo>(io: &mut I) -> Result<Option<Vec<u8>>, PlatformError> {
    let mut header = [0u8; 4];
    let mut offset = 0;
    while offset < header.len() {
        match io.read_some(&mut header[offset..])? {
            Some(n) => offset += n,
            None => {
                if offset == 0 {
                    return Ok(None);
                }
                return Err(PlatformError::AuroraIpcFailed {
                    message: "Aurora daemon closed pipe with a truncated IPC frame header"
                        .to_string(),
                });
            }
        }
    }

    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(PlatformError::AuroraIpcFailed {
            message: format!("Aurora IPC frame exceeds {MAX_FRAME_SIZE} byte limit"),
        });
    }

    let mut payload = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        match io.read_some(&mut payload[filled..])? {
            Some(n) => filled += n,
            None => {
                return Err(PlatformError::AuroraIpcFailed {
                    message: format!(
                        "Aurora daemon closed pipe mid-frame (read {filled} of {len} payload bytes)"
                    ),
                });
            }
        }
    }
    Ok(Some(payload))
}

/// Send `request_json` to a running Aurora daemon and return the JSON response.
pub fn send_aurora_ipc_message(request_json: &str) -> Result<String, PlatformError> {
    let path = pipe_path_for_session(current_session_id()?);
    let handle = open_pipe(&path)?;
    let result = send_and_read(handle, request_json);
    unsafe { CloseHandle(handle) };
    result
}

fn send_and_read(mut handle: HANDLE, request_json: &str) -> Result<String, PlatformError> {
    write_frame(&mut handle, request_json.as_bytes())?;

    let flush_ok = unsafe { FlushFileBuffers(handle) };
    if flush_ok == 0 {
        let code = unsafe { GetLastError() };
        return Err(PlatformError::AuroraIpcFailed {
            message: format!("Failed to flush Aurora IPC request (error {code})"),
        });
    }

    let response = read_frame(&mut handle)?.ok_or_else(|| PlatformError::AuroraIpcFailed {
        message: "aurora daemon closed IPC pipe without a response".to_string(),
    })?;
    String::from_utf8(response).map_err(|e| PlatformError::AuroraIpcFailed {
        message: format!("Aurora returned non-UTF-8 response: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory byte stream for framing tests. `chunk` caps each `read_some`
    /// so short-read handling is exercised deterministically.
    struct MemPipe {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }

    impl MemPipe {
        fn new(data: Vec<u8>) -> Self {
            Self {
                data,
                pos: 0,
                chunk: usize::MAX,
            }
        }
    }

    impl PipeIo for MemPipe {
        fn write_all_bytes(&mut self, buf: &[u8]) -> Result<(), PlatformError> {
            self.data.extend_from_slice(buf);
            Ok(())
        }

        fn read_some(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PlatformError> {
            if self.pos >= self.data.len() {
                return Ok(None);
            }
            let n = buf.len().min(self.data.len() - self.pos).min(self.chunk);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(Some(n))
        }
    }

    #[test]
    fn pipe_name_is_scoped_to_session() {
        assert_eq!(pipe_path_for_session(0), r"\\.\pipe\aurora-0");
        assert_eq!(pipe_path_for_session(42), r"\\.\pipe\aurora-42");
        assert_ne!(pipe_path_for_session(1), pipe_path_for_session(2));
    }

    #[test]
    fn frame_encode_matches_u32_le_header() {
        let payload = br#"{"type":"next"}"#.to_vec();
        let mut sink = MemPipe::new(Vec::new());
        write_frame(&mut sink, &payload).unwrap();
        let expected: Vec<u8> = (payload.len() as u32)
            .to_le_bytes()
            .iter()
            .copied()
            .chain(payload.iter().copied())
            .collect();
        assert_eq!(sink.data, expected);
    }

    #[test]
    fn frame_roundtrip_handles_short_reads() {
        let payload = br#"{"ok":true}"#.to_vec();
        let mut sink = MemPipe::new(Vec::new());
        write_frame(&mut sink, &payload).unwrap();
        let mut pipe = MemPipe::new(sink.data);
        pipe.chunk = 1; // force header + payload assembly one byte at a time
        assert_eq!(read_frame(&mut pipe).unwrap(), Some(payload));
    }

    #[test]
    fn frame_limit_rejects_oversized_payload() {
        let oversized = vec![0u8; MAX_FRAME_SIZE + 1];
        let mut sink = MemPipe::new(Vec::new());
        let write_error = write_frame(&mut sink, &oversized).unwrap_err();
        assert!(write_error.to_string().contains("exceeds"));

        let mut pipe = MemPipe::new((MAX_FRAME_SIZE as u32 + 1).to_le_bytes().to_vec());
        let read_error = read_frame(&mut pipe).unwrap_err();
        assert!(read_error.to_string().contains("exceeds"));
    }

    #[test]
    fn clean_eof_before_header_is_ok_none() {
        let mut pipe = MemPipe::new(Vec::new());
        assert_eq!(read_frame(&mut pipe).unwrap(), None);
    }

    #[test]
    fn truncated_header_is_an_error() {
        let mut pipe = MemPipe::new(vec![1, 0]);
        assert!(read_frame(&mut pipe).is_err());
    }

    #[test]
    fn truncated_payload_is_an_error() {
        let payload = br#"{"type":"next"}"#;
        let mut encoded: Vec<u8> = (payload.len() as u32).to_le_bytes().to_vec();
        encoded.extend_from_slice(&payload[..3]); // cut the payload short
        let mut pipe = MemPipe::new(encoded);
        assert!(read_frame(&mut pipe).is_err());
    }

    #[test]
    fn response_shapes_are_parseable() {
        // The daemon replies {"success":true,"result":…} or
        // {"success":false,"error":…}; send_aurora_ipc_message hands the raw
        // JSON to callers, who check `success`. Sanity-check both shapes.
        let ok: serde_json::Value =
            serde_json::from_str(r#"{"success":true,"result":{"running":true}}"#).unwrap();
        assert_eq!(ok["success"], true);

        let err: serde_json::Value = serde_json::from_str(
            r#"{"success":false,"error":"Invalid message: unknown variant `rate`"}"#,
        )
        .unwrap();
        assert_eq!(err["success"], false);
        assert!(err["error"].as_str().unwrap().contains("Invalid message"));
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later

//! Recording and replaying HID traffic.
//!
//! A capture is a text file with one HID operation per line, so it's easy to
//! read and diff:
//!
//! ```text
//! # Comments start with '#'.
//! model mk2
//! write 02 07 03 01 e4 01 01 00 ab*484 00*532
//! feature-set 03 08 32 00*29
//! feature-get 06 32 -> 06 0c 41 4c 31 32 00*20
//! read 19 -> 01 00 0f 00 00*15
//! read 19 -> timeout
//! ```
//!
//! Bytes are hex; `ab*484` means the byte `ab` repeated 484 times. `write` and
//! `feature-set` are reports sent to the device; `feature-get` and `read` give
//! the requested ID and/or length, then what the device answered.
//!
//! [`Recorder`] wraps a real transport and writes such a file (leaving out
//! reads that timed out); [`Replay`] plays one back as a fake device, failing
//! on the first operation that differs from the recording.

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::{Error, Model, Transport};

/// Runs of at least this many identical bytes are written as `xx*N`.
const MIN_RUN: usize = 4;

/// One HID operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Output report sent to the device.
    Write(Vec<u8>),
    /// Feature report sent to the device.
    FeatureSet(Vec<u8>),
    /// Feature report read from the device.
    FeatureGet {
        /// Requested report ID.
        report_id: u8,
        /// Requested length.
        len: usize,
        /// What the device returned.
        response: Vec<u8>,
    },
    /// Input report read from the device.
    Read {
        /// Requested maximum length.
        len: usize,
        /// What the device returned, or `None` on timeout.
        response: Option<Vec<u8>>,
    },
}

impl Op {
    fn kind(&self) -> &'static str {
        match self {
            Op::Write(_) => "write",
            Op::FeatureSet(_) => "feature-set",
            Op::FeatureGet { .. } => "feature-get",
            Op::Read { .. } => "read",
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Write(data) | Op::FeatureSet(data) => {
                write!(f, "{} {}", self.kind(), HexBytes(data))
            }
            Op::FeatureGet {
                report_id,
                len,
                response,
            } => write!(
                f,
                "feature-get {report_id:02x} {len} -> {}",
                HexBytes(response)
            ),
            Op::Read { len, response } => match response {
                Some(data) => write!(f, "read {len} -> {}", HexBytes(data)),
                None => write!(f, "read {len} -> timeout"),
            },
        }
    }
}

/// Formats bytes as capture hex, compressing runs.
struct HexBytes<'a>(&'a [u8]);

impl fmt::Display for HexBytes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        let mut rest = self.0;
        while let Some(&byte) = rest.first() {
            let run = rest.iter().take_while(|&&b| b == byte).count();
            let count = if run >= MIN_RUN { run } else { 1 };
            if !out.is_empty() {
                out.push(' ');
            }
            write!(out, "{byte:02x}")?;
            if count > 1 {
                write!(out, "*{count}")?;
            }
            rest = &rest[count..];
        }
        f.write_str(&out)
    }
}

/// A parsed capture file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capture {
    /// Model named by the `model` line, if any.
    pub model: Option<Model>,
    /// Operations in order.
    pub ops: Vec<Op>,
}

impl Capture {
    /// Parses capture text.
    pub fn parse(text: &str) -> Result<Capture, Error> {
        let mut capture = Capture::default();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let error = |message: String| Error::Capture(format!("line {}: {message}", index + 1));
            let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
            let op = match kind {
                "model" => {
                    let id = rest.trim();
                    capture.model = Some(
                        Model::from_id(id).ok_or_else(|| error(format!("unknown model {id:?}")))?,
                    );
                    continue;
                }
                "write" => Op::Write(parse_hex(rest).map_err(error)?),
                "feature-set" => Op::FeatureSet(parse_hex(rest).map_err(error)?),
                "feature-get" => {
                    let (request, response) = split_arrow(rest).map_err(error)?;
                    let mut words = request.split_whitespace();
                    let (Some(id), Some(len), None) = (words.next(), words.next(), words.next())
                    else {
                        return Err(error("expected 'feature-get <id> <len> -> <bytes>'".into()));
                    };
                    Op::FeatureGet {
                        report_id: u8::from_str_radix(id, 16)
                            .map_err(|e| error(format!("bad report ID {id:?}: {e}")))?,
                        len: parse_len(len).map_err(error)?,
                        response: parse_hex(response).map_err(error)?,
                    }
                }
                "read" => {
                    let (len, response) = split_arrow(rest).map_err(error)?;
                    Op::Read {
                        len: parse_len(len.trim()).map_err(error)?,
                        response: match response.trim() {
                            "timeout" => None,
                            bytes => Some(parse_hex(bytes).map_err(error)?),
                        },
                    }
                }
                other => return Err(error(format!("unknown operation {other:?}"))),
            };
            capture.ops.push(op);
        }
        Ok(capture)
    }
}

impl fmt::Display for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(model) = self.model {
            writeln!(f, "model {}", model.id())?;
        }
        for op in &self.ops {
            writeln!(f, "{op}")?;
        }
        Ok(())
    }
}

fn split_arrow(text: &str) -> Result<(&str, &str), String> {
    text.split_once("->")
        .ok_or_else(|| "expected '->' before the response".to_owned())
}

fn parse_len(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|e| format!("bad length {text:?}: {e}"))
}

fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let (byte, count) = match token.split_once('*') {
            Some((byte, count)) => (
                byte,
                count
                    .parse::<usize>()
                    .map_err(|e| format!("bad repeat count in {token:?}: {e}"))?,
            ),
            None => (token, 1),
        };
        if byte.len() != 2 {
            return Err(format!("expected two hex digits, got {token:?}"));
        }
        let byte = u8::from_str_radix(byte, 16).map_err(|e| format!("bad byte {token:?}: {e}"))?;
        out.extend(std::iter::repeat_n(byte, count));
    }
    Ok(out)
}

/// A fake device that replays a capture.
///
/// Every call must match the next recorded operation. Clones share their
/// position, so a test can keep one to call [`Replay::finish`] after handing
/// another to a [`Device`](crate::Device).
#[derive(Clone)]
pub struct Replay {
    state: Arc<Mutex<ReplayState>>,
}

struct ReplayState {
    ops: VecDeque<Op>,
    position: usize,
}

impl Replay {
    /// Replays `ops` in order.
    #[must_use]
    pub fn new(ops: impl IntoIterator<Item = Op>) -> Replay {
        Replay {
            state: Arc::new(Mutex::new(ReplayState {
                ops: ops.into_iter().collect(),
                position: 0,
            })),
        }
    }

    /// Fails unless every recorded operation was replayed.
    pub fn finish(&self) -> Result<(), Error> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match state.ops.front() {
            None => Ok(()),
            Some(op) => Err(Error::Capture(format!(
                "operation {} was never performed: {op}",
                state.position + 1
            ))),
        }
    }

    /// Pops the next operation, checking that it matches `actual`.
    fn next(&self, actual: &Op) -> Result<Op, Error> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.position += 1;
        let position = state.position;
        let Some(expected) = state.ops.pop_front() else {
            return Err(Error::Capture(format!(
                "operation {position}: capture ended, but got {actual}"
            )));
        };
        let matches = match (&expected, actual) {
            (Op::Write(a), Op::Write(b)) | (Op::FeatureSet(a), Op::FeatureSet(b)) => a == b,
            (
                Op::FeatureGet { report_id, len, .. },
                Op::FeatureGet {
                    report_id: actual_id,
                    len: actual_len,
                    ..
                },
            ) => report_id == actual_id && len == actual_len,
            (
                Op::Read { len, .. },
                Op::Read {
                    len: actual_len, ..
                },
            ) => len == actual_len,
            _ => false,
        };
        if matches {
            Ok(expected)
        } else {
            Err(Error::Capture(mismatch(position, &expected, actual)))
        }
    }
}

fn mismatch(position: usize, expected: &Op, actual: &Op) -> String {
    let mut message = format!("operation {position}: expected {expected}\n  but got {actual}");
    if let (Op::Write(a) | Op::FeatureSet(a), Op::Write(b) | Op::FeatureSet(b)) = (expected, actual)
    {
        let first_difference = a.iter().zip(b).position(|(x, y)| x != y);
        match first_difference {
            Some(at) => {
                let _ = write!(
                    message,
                    "\n  first difference at byte {at}: {:02x} != {:02x}",
                    a[at], b[at]
                );
            }
            None => {
                let _ = write!(message, "\n  lengths differ: {} != {}", a.len(), b.len());
            }
        }
    }
    message
}

impl Transport for Replay {
    fn write(&mut self, report: &[u8]) -> Result<(), Error> {
        self.next(&Op::Write(report.to_vec())).map(drop)
    }

    fn send_feature_report(&mut self, report: &[u8]) -> Result<(), Error> {
        self.next(&Op::FeatureSet(report.to_vec())).map(drop)
    }

    fn get_feature_report(&mut self, report_id: u8, len: usize) -> Result<Vec<u8>, Error> {
        let actual = Op::FeatureGet {
            report_id,
            len,
            response: Vec::new(),
        };
        match self.next(&actual)? {
            Op::FeatureGet { response, .. } => Ok(response),
            _ => unreachable!("next() checked the kind"),
        }
    }

    /// Reads time out unless the next recorded operation is a read, since
    /// recordings leave out timeouts.
    fn read(&mut self, len: usize, _timeout: Option<Duration>) -> Result<Option<Vec<u8>>, Error> {
        let next_is_read = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            matches!(state.ops.front(), Some(Op::Read { .. }))
        };
        if !next_is_read {
            return Ok(None);
        }
        match self.next(&Op::Read {
            len,
            response: None,
        })? {
            Op::Read { response, .. } => Ok(response),
            _ => unreachable!("next() checked the kind"),
        }
    }
}

/// Wraps a transport and writes every operation to a capture file.
pub struct Recorder<T> {
    inner: T,
    out: Box<dyn Write + Send>,
}

impl<T: Transport> Recorder<T> {
    /// Starts recording. Writes a header naming `model` right away.
    pub fn new(inner: T, model: Model, mut out: Box<dyn Write + Send>) -> Result<Self, Error> {
        writeln!(out, "# SuperDeck HID capture of a {model}")?;
        writeln!(out, "model {}", model.id())?;
        Ok(Self { inner, out })
    }

    fn record<R>(
        &mut self,
        result: Result<R, Error>,
        op: impl FnOnce(&R) -> Op,
    ) -> Result<R, Error> {
        match &result {
            Ok(value) => writeln!(self.out, "{}", op(value))?,
            Err(error) => writeln!(self.out, "# error: {error}")?,
        }
        self.out.flush()?;
        result
    }
}

impl<T: Transport> Transport for Recorder<T> {
    fn write(&mut self, report: &[u8]) -> Result<(), Error> {
        let result = self.inner.write(report);
        self.record(result, |()| Op::Write(report.to_vec()))
    }

    fn send_feature_report(&mut self, report: &[u8]) -> Result<(), Error> {
        let result = self.inner.send_feature_report(report);
        self.record(result, |()| Op::FeatureSet(report.to_vec()))
    }

    fn get_feature_report(&mut self, report_id: u8, len: usize) -> Result<Vec<u8>, Error> {
        let result = self.inner.get_feature_report(report_id, len);
        self.record(result, |response| Op::FeatureGet {
            report_id,
            len,
            response: response.clone(),
        })
    }

    fn read(&mut self, len: usize, timeout: Option<Duration>) -> Result<Option<Vec<u8>>, Error> {
        let result = self.inner.read(len, timeout);
        // Timeouts carry no information and would flood an idle recording.
        if matches!(result, Ok(None)) {
            return result;
        }
        self.record(result, |response| Op::Read {
            len,
            response: response.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_runs_round_trip() {
        let data = [0x02, 0x07, 0, 0, 0, 0, 0xab, 0xab, 0xab, 0xff];
        let text = HexBytes(&data).to_string();
        assert_eq!(text, "02 07 00*4 ab ab ab ff");
        assert_eq!(parse_hex(&text).unwrap(), data);
        assert_eq!(parse_hex("").unwrap(), Vec::<u8>::new());
        assert!(parse_hex("0").is_err());
        assert!(parse_hex("zz").is_err());
        assert!(parse_hex("00*x").is_err());
    }

    #[test]
    fn capture_round_trips() {
        let text = "\
# header
model neo
write 02 07 00 01 03 00 00 00 aa bb cc 00*1013
feature-set 03 08 32 00*29
feature-get 06 32 -> 06 0c 41 42 00*28
read 14 -> 01 00 08 00 00*10
read 14 -> timeout
";
        let capture = Capture::parse(text).unwrap();
        assert_eq!(capture.model, Some(Model::Neo));
        assert_eq!(capture.ops.len(), 5);
        let again = Capture::parse(&capture.to_string()).unwrap();
        assert_eq!(again, capture);
    }

    #[test]
    fn parse_errors_name_the_line() {
        let error = Capture::parse("model mk2\nfrobnicate 00").unwrap_err();
        assert!(error.to_string().contains("line 2"), "{error}");
        assert!(Capture::parse("model nope").is_err());
        assert!(Capture::parse("read 14").is_err());
        assert!(Capture::parse("feature-get 06 -> 00").is_err());
    }

    #[test]
    fn replay_checks_every_operation() {
        let mut replay = Replay::new([
            Op::Write(vec![1, 2, 3]),
            Op::Read {
                len: 4,
                response: Some(vec![9]),
            },
        ]);
        let handle = replay.clone();
        replay.write(&[1, 2, 3]).unwrap();
        assert!(handle.finish().is_err());
        assert_eq!(replay.read(4, None).unwrap(), Some(vec![9]));
        handle.finish().unwrap();
        assert_eq!(replay.read(4, None).unwrap(), None);
        assert!(replay.write(&[1]).is_err());

        let mut replay = Replay::new([Op::Write(vec![1, 2, 3])]);
        let error = replay.write(&[1, 5, 3]).unwrap_err().to_string();
        assert!(error.contains("first difference at byte 1"), "{error}");
    }

    #[test]
    fn recorder_writes_a_replayable_capture() {
        #[derive(Clone, Default)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let ops = vec![
            Op::FeatureSet(vec![3, 8, 50]),
            Op::FeatureGet {
                report_id: 6,
                len: 32,
                response: vec![6, 2, 0x41, 0x42],
            },
            Op::Read {
                len: 19,
                response: None,
            },
            Op::Read {
                len: 19,
                response: Some(vec![1, 0, 15, 0]),
            },
        ];
        let sink = Sink::default();
        let mut recorder =
            Recorder::new(Replay::new(ops.clone()), Model::Mk2, Box::new(sink.clone())).unwrap();
        recorder.send_feature_report(&[3, 8, 50]).unwrap();
        recorder.get_feature_report(6, 32).unwrap();
        assert_eq!(recorder.read(19, None).unwrap(), None);
        recorder.read(19, None).unwrap();

        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let capture = Capture::parse(&text).unwrap();
        assert_eq!(capture.model, Some(Model::Mk2));
        // The timeout isn't recorded.
        let expected: Vec<Op> = ops
            .into_iter()
            .filter(|op| {
                op != &Op::Read {
                    len: 19,
                    response: None,
                }
            })
            .collect();
        assert_eq!(capture.ops, expected);
    }
}

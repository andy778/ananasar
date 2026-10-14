//! File-backed server data log: numbers entries, times them and writes the
//! text produced by [`ananasar_core::log`].

use ananasar_core::log::{Entry, LogMode, Response, file_name, format_entry};
use ananasar_core::sync::lock;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

struct State {
    /// Where entries go; `None` while recording is off.
    out: Option<Box<dyn Write + Send>>,
    path: Option<PathBuf>,
    mode: LogMode,
    started: Instant,
    /// Entries written to the current recording.
    number: u64,
    /// Bytes written to the current recording.
    bytes: u64,
    /// Connection slots (1 and 2) currently in use. Kept across recordings so
    /// a recording started mid-connection still shows the right number.
    used: [bool; 2],
}

/// Records the server's Modbus exchanges, per connection, to a log file. It
/// can be created already recording ([`create`](Self::create)) or idle
/// ([`idle`](Self::idle)) and started and stopped while the server runs.
pub struct LogSink {
    state: Mutex<State>,
}

impl LogSink {
    /// A sink that is not recording yet.
    pub fn idle() -> Self {
        Self {
            state: Mutex::new(State {
                out: None,
                path: None,
                mode: LogMode::Raw,
                started: Instant::now(),
                number: 0,
                bytes: 0,
                used: [false; 2],
            }),
        }
    }

    /// Create `ananasar-<label>-<time>.log` in `dir` (which must exist) and log
    /// into it. The time in the name is UTC.
    pub fn create(dir: &Path, label: &str, mode: LogMode) -> io::Result<Self> {
        let sink = Self::idle();
        sink.start(dir, label, mode)?;
        Ok(sink)
    }

    /// Log into any writer (used by tests and embedders).
    pub fn from_writer(out: Box<dyn Write + Send>, mode: LogMode) -> Self {
        let sink = Self::idle();
        sink.start_writer(out, None, mode);
        sink
    }

    /// Start recording into a new file in `dir`, replacing any current
    /// recording. Returns the file's path.
    pub fn start(&self, dir: &Path, label: &str, mode: LogMode) -> io::Result<PathBuf> {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let path = dir.join(file_name(label, secs));
        let file = File::create(&path)?;
        self.start_writer(Box::new(file), Some(path.clone()), mode);
        Ok(path)
    }

    fn start_writer(&self, out: Box<dyn Write + Send>, path: Option<PathBuf>, mode: LogMode) {
        let mut st = lock(&self.state);
        st.out = Some(out);
        st.path = path;
        st.mode = mode;
        st.started = Instant::now();
        st.number = 0;
        st.bytes = 0;
    }

    /// Stop recording and close the file. The statistics stay readable.
    pub fn stop(&self) {
        lock(&self.state).out = None;
    }

    /// True while entries are being written.
    pub fn is_recording(&self) -> bool {
        lock(&self.state).out.is_some()
    }

    /// The current or last log file, if the sink wrote to one.
    pub fn path(&self) -> Option<PathBuf> {
        lock(&self.state).path.clone()
    }

    /// Entries written to the current or last recording.
    pub fn recordings(&self) -> u64 {
        lock(&self.state).number
    }

    /// Bytes written to the current or last recording.
    pub fn bytes(&self) -> u64 {
        lock(&self.state).bytes
    }

    /// Claim the lowest free connection number (1 or 2). Returns 0 if both
    /// are taken, which the server's client limit prevents.
    pub fn open_connection(&self) -> u8 {
        let mut st = lock(&self.state);
        match st.used.iter().position(|u| !u) {
            Some(i) => {
                st.used[i] = true;
                i as u8 + 1
            }
            None => 0,
        }
    }

    /// Release a number from [`open_connection`](Self::open_connection).
    pub fn close_connection(&self, connection: u8) {
        let mut st = lock(&self.state);
        if let Some(used) = usize::from(connection).checked_sub(1).and_then(|i| st.used.get_mut(i)) {
            *used = false;
        }
    }

    /// Append one entry while recording; otherwise do nothing. Write errors
    /// are ignored: a full disk must not stop the server answering.
    pub fn record(&self, connection: u8, query: &[u8], response: Response<'_>) {
        let mut st = lock(&self.state);
        if st.out.is_none() {
            return;
        }
        st.number += 1;
        let elapsed_ms = u64::try_from(st.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let entry = Entry { number: st.number, elapsed_ms, connection, query, response };
        let text = format_entry(&entry, st.mode);
        st.bytes += text.len() as u64;
        if let Some(out) = st.out.as_mut() {
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            lock(&self.0).extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn numbers_entries_and_connections() {
        let buf = Buf::default();
        let sink = LogSink::from_writer(Box::new(buf.clone()), LogMode::Raw);
        assert_eq!(sink.open_connection(), 1);
        assert_eq!(sink.open_connection(), 2);
        assert_eq!(sink.open_connection(), 0);
        sink.close_connection(1);
        assert_eq!(sink.open_connection(), 1);
        sink.record(1, &[1, 2], Response::Error("x"));
        sink.record(2, &[], Response::Error("y"));
        let text = String::from_utf8(lock(&buf.0).clone()).unwrap();
        assert!(text.starts_with("#1 "));
        assert!(text.contains("\n#2 "));
        assert!(text.contains("Connection: 2\n"));
    }

    #[test]
    fn can_start_and_stop_while_connections_stay_numbered() {
        let sink = LogSink::idle();
        assert_eq!(sink.open_connection(), 1);
        sink.record(1, &[], Response::Error("not recording"));
        assert_eq!(sink.recordings(), 0);
        let buf = Buf::default();
        sink.start_writer(Box::new(buf.clone()), None, LogMode::Raw);
        assert!(sink.is_recording());
        sink.record(1, &[], Response::Error("a"));
        sink.stop();
        sink.record(1, &[], Response::Error("b"));
        assert!(!sink.is_recording());
        assert_eq!(sink.recordings(), 1);
        assert!(sink.bytes() > 0);
        let text = String::from_utf8(lock(&buf.0).clone()).unwrap();
        assert!(text.contains("Connection: 1\n"));
        assert_eq!(text.matches("#1 ").count(), 1);
        assert!(!text.contains("#2 "));
    }
}

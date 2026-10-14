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
    out: Box<dyn Write + Send>,
    number: u64,
    /// Connection slots (1 and 2) currently in use.
    used: [bool; 2],
}

/// Records the server's Modbus exchanges, per connection, to one log file.
pub struct LogSink {
    mode: LogMode,
    path: Option<PathBuf>,
    started: Instant,
    state: Mutex<State>,
}

impl LogSink {
    /// Create `<ip1> & <ip2> & <time>.log` in `dir` (which must exist) and log
    /// into it. The time in the name is UTC.
    pub fn create(dir: &Path, ip1: &str, ip2: &str, mode: LogMode) -> io::Result<Self> {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let path = dir.join(file_name(ip1, ip2, secs));
        let file = File::create(&path)?;
        let mut sink = Self::from_writer(Box::new(file), mode);
        sink.path = Some(path);
        Ok(sink)
    }

    /// Log into any writer (used by tests and embedders).
    pub fn from_writer(out: Box<dyn Write + Send>, mode: LogMode) -> Self {
        Self { mode, path: None, started: Instant::now(), state: Mutex::new(State { out, number: 0, used: [false; 2] }) }
    }

    /// The log file, if this sink writes to one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
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

    /// Append one entry. Write errors are ignored: a full disk must not stop
    /// the server answering.
    pub fn record(&self, connection: u8, query: &[u8], response: Response<'_>) {
        let mut st = lock(&self.state);
        st.number += 1;
        let elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let entry = Entry { number: st.number, elapsed_ms, connection, query, response };
        let text = format_entry(&entry, self.mode);
        let _ = st.out.write_all(text.as_bytes());
        let _ = st.out.flush();
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
}

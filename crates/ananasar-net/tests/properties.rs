//! Property tests for the frame reader: arbitrary bytes never panic it, and a
//! frame comes out whole however the stream is cut into reads.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_core::protocol::build;
use ananasar_net::read_frame;
use proptest::prelude::*;
use std::io::{self, Cursor, Read};

/// Hands out at most `chunk` bytes per read, like a slow TCP stream.
struct Chunked<'a> {
    data: &'a [u8],
    chunk: usize,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.chunk.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic_the_frame_reader(bytes in proptest::collection::vec(any::<u8>(), 0..1500)) {
        if let Ok(frame) = read_frame(&mut Cursor::new(&bytes)) {
            // A frame is the prefix the length field declares.
            let declared = usize::from(u16::from_be_bytes([frame[4], frame[5]]));
            prop_assert_eq!(frame.len(), 6 + declared);
            prop_assert_eq!(&frame[..], &bytes[..frame.len()]);
        }
    }

    #[test]
    fn frames_survive_any_segmentation(
        txn in any::<u16>(),
        unit in any::<u8>(),
        function in any::<u8>(),
        data in proptest::collection::vec(any::<u8>(), 0..=250),
        trailing in proptest::collection::vec(any::<u8>(), 0..20),
        chunk in 1usize..40,
    ) {
        let wire = build(txn, unit, function, &data);
        let stream = [wire.clone(), trailing].concat();
        let got = read_frame(&mut Chunked { data: &stream, chunk }).unwrap();
        prop_assert_eq!(got, wire);
    }

    #[test]
    fn two_pipelined_frames_read_back_in_order(
        a in proptest::collection::vec(any::<u8>(), 0..100),
        b in proptest::collection::vec(any::<u8>(), 0..100),
    ) {
        let (fa, fb) = (build(1, 1, 3, &a), build(2, 1, 4, &b));
        let mut stream = Cursor::new([fa.clone(), fb.clone()].concat());
        prop_assert_eq!(read_frame(&mut stream).unwrap(), fa);
        prop_assert_eq!(read_frame(&mut stream).unwrap(), fb);
    }
}

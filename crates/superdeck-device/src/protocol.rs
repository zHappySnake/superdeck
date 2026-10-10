// SPDX-License-Identifier: GPL-3.0-or-later

//! The Stream Deck HID wire protocol, as pure functions from commands to
//! report bytes and from report bytes to key states. No I/O happens here.
//!
//! Every report starts with its HID report ID. See `docs/device-protocol.md`.

/// Wire protocol generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Stream Deck (original, 2017): BMP images in two 8191-byte reports,
    /// keys numbered right to left.
    Original,
    /// Stream Deck Mini family: BMP images in 1024-byte reports.
    Mini,
    /// Everything since the Stream Deck XL: JPEG images in 1024-byte reports,
    /// 32-byte feature reports.
    Gen2,
}

/// A feature report to read and where the answer starts in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FeatureQuery {
    pub report_id: u8,
    pub len: usize,
    pub offset: usize,
}

/// Output report ID used for key images.
const IMAGE_REPORT_ID: u8 = 0x02;
/// Input report ID used for key states.
const INPUT_REPORT_ID: u8 = 0x01;

impl Protocol {
    fn image_report_len(self) -> usize {
        match self {
            Protocol::Original => 8191,
            Protocol::Mini | Protocol::Gen2 => 1024,
        }
    }

    fn image_header_len(self) -> usize {
        match self {
            Protocol::Original | Protocol::Mini => 16,
            Protocol::Gen2 => 8,
        }
    }

    fn feature_report_len(self) -> usize {
        match self {
            Protocol::Original | Protocol::Mini => 17,
            Protocol::Gen2 => 32,
        }
    }

    fn input_header_len(self) -> usize {
        match self {
            Protocol::Original | Protocol::Mini => 1,
            Protocol::Gen2 => 4,
        }
    }

    /// Largest encoded image that fits the page counter.
    pub(crate) fn max_image_len(self) -> usize {
        let max_pages = match self {
            // One byte, and the original counts from 1.
            Protocol::Original | Protocol::Mini => usize::from(u8::MAX),
            Protocol::Gen2 => usize::from(u16::MAX) + 1,
        };
        max_pages * (self.image_report_len() - self.image_header_len())
    }

    /// Splits an encoded key image into output reports. `key` is the
    /// device's own key index (see [`Protocol::device_key`]). The image must
    /// be at most [`Protocol::max_image_len`] bytes.
    pub(crate) fn image_reports(self, key: u8, image: &[u8]) -> Vec<Vec<u8>> {
        let report_len = self.image_report_len();
        let payload_len = report_len - self.image_header_len();
        let page_count = image.len().div_ceil(payload_len).max(1);

        (0..page_count)
            .map(|page| {
                let start = page * payload_len;
                let chunk = &image[start..image.len().min(start + payload_len)];
                let last = u8::from(page + 1 == page_count);
                let mut report = Vec::with_capacity(report_len);
                match self {
                    Protocol::Original | Protocol::Mini => {
                        // The original numbers pages from 1, the Mini from 0.
                        let first_page = u8::from(self == Protocol::Original);
                        let page = u8::try_from(page).expect("image too large") + first_page;
                        report.extend_from_slice(&[
                            IMAGE_REPORT_ID,
                            0x01,
                            page,
                            0x00,
                            last,
                            key + 1,
                        ]);
                    }
                    Protocol::Gen2 => {
                        let [len_lo, len_hi] = u16::try_from(chunk.len())
                            .expect("chunk fits in a report")
                            .to_le_bytes();
                        let [page_lo, page_hi] =
                            u16::try_from(page).expect("image too large").to_le_bytes();
                        report.extend_from_slice(&[
                            IMAGE_REPORT_ID,
                            0x07,
                            key,
                            last,
                            len_lo,
                            len_hi,
                            page_lo,
                            page_hi,
                        ]);
                    }
                }
                report.resize(self.image_header_len(), 0);
                report.extend_from_slice(chunk);
                report.resize(report_len, 0);
                report
            })
            .collect()
    }

    /// An empty image report. Sending one when opening the device discards
    /// any image transfer a previous program left half-finished.
    pub(crate) fn reset_key_stream_report(self) -> Vec<u8> {
        let mut report = vec![0; self.image_report_len()];
        report[0] = IMAGE_REPORT_ID;
        report
    }

    /// Feature report setting the backlight brightness (0–100 %).
    pub(crate) fn brightness_report(self, percent: u8) -> Vec<u8> {
        let percent = percent.min(100);
        let header: &[u8] = match self {
            Protocol::Original | Protocol::Mini => &[0x05, 0x55, 0xaa, 0xd1, 0x01, percent],
            Protocol::Gen2 => &[0x03, 0x08, percent],
        };
        self.feature_report(header)
    }

    /// Feature report that shows the boot logo and clears every key.
    pub(crate) fn reset_report(self) -> Vec<u8> {
        let header: &[u8] = match self {
            Protocol::Original | Protocol::Mini => &[0x0b, 0x63],
            Protocol::Gen2 => &[0x03, 0x02],
        };
        self.feature_report(header)
    }

    fn feature_report(self, header: &[u8]) -> Vec<u8> {
        let mut report = header.to_vec();
        report.resize(self.feature_report_len(), 0);
        report
    }

    /// Where to find the serial number.
    pub(crate) fn serial_number_query(self) -> FeatureQuery {
        match self {
            Protocol::Original | Protocol::Mini => FeatureQuery {
                report_id: 0x03,
                len: 17,
                offset: 5,
            },
            Protocol::Gen2 => FeatureQuery {
                report_id: 0x06,
                len: 32,
                offset: 2,
            },
        }
    }

    /// Where to find the firmware version.
    pub(crate) fn firmware_version_query(self) -> FeatureQuery {
        match self {
            Protocol::Original | Protocol::Mini => FeatureQuery {
                report_id: 0x04,
                len: 17,
                offset: 5,
            },
            Protocol::Gen2 => FeatureQuery {
                report_id: 0x05,
                len: 32,
                offset: 6,
            },
        }
    }

    /// Size of the input report carrying key states.
    pub(crate) fn input_report_len(self, keys: u8, extra_inputs: u8) -> usize {
        self.input_header_len() + usize::from(keys) + usize::from(extra_inputs)
    }

    /// Decodes a key state report into one entry per key, in the device's
    /// own key order. Returns `None` for reports that aren't key states.
    pub(crate) fn parse_key_states(self, report: &[u8], keys: u8) -> Option<Vec<bool>> {
        if report.first() != Some(&INPUT_REPORT_ID) {
            return None;
        }
        let start = self.input_header_len();
        let states = report.get(start..start + usize::from(keys))?;
        Some(states.iter().map(|&state| state != 0).collect())
    }

    /// Maps a key index (row-major from the top left, as SuperDeck numbers
    /// keys) to the index the device uses on the wire. Only the original
    /// numbers its keys right to left; the mapping is its own inverse.
    pub(crate) fn device_key(self, key: u8, columns: u8) -> u8 {
        match self {
            Protocol::Original => {
                let column = key % columns;
                key - column + (columns - 1 - column)
            }
            Protocol::Mini | Protocol::Gen2 => key,
        }
    }
}

/// Reads a NUL-terminated ASCII string out of a feature report.
pub(crate) fn extract_string(report: &[u8], offset: usize) -> String {
    let bytes = report.get(offset..).unwrap_or_default();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen2_image_is_split_into_pages() {
        let image = vec![0xab; 1500];
        let reports = Protocol::Gen2.image_reports(3, &image);
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|r| r.len() == 1024));
        // 1016 = 0x03f8 bytes on page 0, 484 = 0x01e4 on page 1.
        assert_eq!(reports[0][..8], [0x02, 0x07, 3, 0, 0xf8, 0x03, 0, 0]);
        assert_eq!(reports[1][..8], [0x02, 0x07, 3, 1, 0xe4, 0x01, 1, 0]);
        assert!(reports[0][8..].iter().all(|&b| b == 0xab));
        assert!(reports[1][8..8 + 484].iter().all(|&b| b == 0xab));
        assert!(reports[1][8 + 484..].iter().all(|&b| b == 0));
    }

    #[test]
    fn gen2_image_filling_a_page_exactly() {
        let reports = Protocol::Gen2.image_reports(0, &[1; 1016]);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0][..8], [0x02, 0x07, 0, 1, 0xf8, 0x03, 0, 0]);
    }

    #[test]
    fn original_bmp_takes_two_pages_numbered_from_one() {
        let image = vec![0x11; 54 + 72 * 72 * 3];
        let reports = Protocol::Original.image_reports(4, &image);
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|r| r.len() == 8191));
        assert_eq!(reports[0][..6], [0x02, 0x01, 1, 0, 0, 5]);
        assert_eq!(reports[1][..6], [0x02, 0x01, 2, 0, 1, 5]);
        assert!(reports[0][6..16].iter().all(|&b| b == 0));
        assert_eq!(reports[1][16 + 15606 - 8175 - 1], 0x11);
        assert_eq!(reports[1][16 + 15606 - 8175], 0);
    }

    #[test]
    fn largest_images_fit_the_page_counter() {
        for protocol in [Protocol::Original, Protocol::Mini, Protocol::Gen2] {
            let reports = protocol.image_reports(0, &vec![0; protocol.max_image_len()]);
            assert!(reports.last().unwrap()[..6].contains(&1), "{protocol:?}");
        }
    }

    #[test]
    fn mini_pages_are_numbered_from_zero() {
        let image = vec![0x22; 54 + 80 * 80 * 3];
        let reports = Protocol::Mini.image_reports(5, &image);
        // 19254 bytes in 1008-byte pages.
        assert_eq!(reports.len(), 20);
        assert_eq!(reports[0][..6], [0x02, 0x01, 0, 0, 0, 6]);
        assert_eq!(reports[19][..6], [0x02, 0x01, 19, 0, 1, 6]);
    }

    #[test]
    fn brightness_and_reset() {
        assert_eq!(
            Protocol::Gen2.brightness_report(42)[..4],
            [0x03, 0x08, 42, 0]
        );
        assert_eq!(Protocol::Gen2.brightness_report(200)[2], 100);
        assert_eq!(Protocol::Gen2.brightness_report(42).len(), 32);
        assert_eq!(
            Protocol::Mini.brightness_report(7)[..7],
            [0x05, 0x55, 0xaa, 0xd1, 0x01, 7, 0]
        );
        assert_eq!(Protocol::Original.brightness_report(7).len(), 17);
        assert_eq!(Protocol::Gen2.reset_report()[..3], [0x03, 0x02, 0]);
        assert_eq!(Protocol::Mini.reset_report()[..3], [0x0b, 0x63, 0]);
    }

    #[test]
    fn key_states() {
        let report = [
            0x01, 0x00, 0x0f, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ];
        let states = Protocol::Gen2.parse_key_states(&report, 15).unwrap();
        assert_eq!(states.iter().filter(|&&s| s).count(), 2);
        assert!(states[1] && states[14]);

        assert_eq!(Protocol::Gen2.parse_key_states(&report[..10], 15), None);
        assert_eq!(
            Protocol::Mini.parse_key_states(&[0x02, 0, 0, 0, 0, 0, 0], 6),
            None
        );
        assert_eq!(
            Protocol::Mini.parse_key_states(&[0x01, 0, 0, 1, 0, 0, 0], 6),
            Some(vec![false, false, true, false, false, false])
        );
    }

    #[test]
    fn original_keys_are_mirrored_per_row() {
        let mapped: Vec<u8> = (0..15)
            .map(|k| Protocol::Original.device_key(k, 5))
            .collect();
        assert_eq!(mapped, [4, 3, 2, 1, 0, 9, 8, 7, 6, 5, 14, 13, 12, 11, 10]);
        for key in 0..15 {
            let device = Protocol::Original.device_key(key, 5);
            assert_eq!(Protocol::Original.device_key(device, 5), key);
        }
        assert_eq!(Protocol::Gen2.device_key(3, 5), 3);
    }

    #[test]
    fn strings() {
        assert_eq!(
            extract_string(b"\x06\x0cAL12K1A01234\0\0\0", 2),
            "AL12K1A01234"
        );
        assert_eq!(
            extract_string(b"\x05\x0c\xfe\xed\xbe\xef1.00.012\0", 6),
            "1.00.012"
        );
        assert_eq!(extract_string(b"\x03", 5), "");
    }
}

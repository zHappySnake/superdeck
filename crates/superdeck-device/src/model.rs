// SPDX-License-Identifier: GPL-3.0-or-later

//! The device models SuperDeck knows how to drive, and what each one needs.
//!
//! The values come from the protocol notes in `docs/device-protocol.md`.

use std::fmt;

use crate::key_image::{ImageFormat, Rotation, Transform};
use crate::protocol::Protocol;

/// USB vendor ID of every Elgato device.
pub const ELGATO_VENDOR_ID: u16 = 0x0fd9;

/// A supported Stream Deck model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Model {
    /// Stream Deck (original, 2017).
    Original,
    /// Stream Deck (original, v2).
    OriginalV2,
    /// Stream Deck MK.2.
    Mk2,
    /// Stream Deck MK.2 with scissor keys.
    Mk2Scissor,
    /// Stream Deck Mini.
    Mini,
    /// Stream Deck Mini MK.2.
    MiniMk2,
    /// Stream Deck XL.
    Xl,
    /// Stream Deck XL (v2).
    XlV2,
    /// Stream Deck Neo. Only the eight keys are supported; the info bar and
    /// the two touch keys come after 1.0.
    Neo,
    /// Stream Deck Module, 6 keys.
    Module6,
    /// Stream Deck Module, 15 keys.
    Module15,
    /// Stream Deck Module, 32 keys.
    Module32,
}

/// Everything that differs between models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// Human-readable product name.
    pub name: &'static str,
    /// USB product ID.
    pub product_id: u16,
    /// Number of key columns.
    pub columns: u8,
    /// Number of key rows.
    pub rows: u8,
    /// Width and height of a key image, in pixels. Keys are square.
    pub key_size: u32,
    /// Image format the device decodes.
    pub image_format: ImageFormat,
    /// How an upright image must be rotated and mirrored before encoding so
    /// it shows upright on the device.
    pub transform: Transform,
    /// Wire protocol generation.
    pub protocol: Protocol,
    /// Extra input bytes after the key states (the Neo reports its two touch
    /// keys there).
    pub(crate) extra_inputs: u8,
}

impl Spec {
    /// Total number of keys.
    #[must_use]
    pub const fn key_count(&self) -> u8 {
        self.columns * self.rows
    }
}

const fn mirrored(rotation: Rotation, flip_horizontal: bool, flip_vertical: bool) -> Transform {
    Transform {
        rotation,
        flip_horizontal,
        flip_vertical,
    }
}

/// 15 keys, 72 px, JPEG, rotated 180°.
const fn gen2_15(name: &'static str, product_id: u16) -> Spec {
    Spec {
        name,
        product_id,
        columns: 5,
        rows: 3,
        key_size: 72,
        image_format: ImageFormat::Jpeg,
        transform: mirrored(Rotation::None, true, true),
        protocol: Protocol::Gen2,
        extra_inputs: 0,
    }
}

/// 32 keys, 96 px, JPEG, rotated 180°.
const fn gen2_32(name: &'static str, product_id: u16) -> Spec {
    Spec {
        name,
        product_id,
        columns: 8,
        rows: 4,
        key_size: 96,
        image_format: ImageFormat::Jpeg,
        transform: mirrored(Rotation::None, true, true),
        protocol: Protocol::Gen2,
        extra_inputs: 0,
    }
}

/// 6 keys, 80 px, BMP, transposed.
const fn mini(name: &'static str, product_id: u16) -> Spec {
    Spec {
        name,
        product_id,
        columns: 3,
        rows: 2,
        key_size: 80,
        image_format: ImageFormat::Bmp,
        transform: mirrored(Rotation::Ccw90, false, true),
        protocol: Protocol::Mini,
        extra_inputs: 0,
    }
}

const ORIGINAL: Spec = Spec {
    name: "Stream Deck",
    product_id: 0x0060,
    columns: 5,
    rows: 3,
    key_size: 72,
    image_format: ImageFormat::Bmp,
    transform: mirrored(Rotation::None, true, true),
    protocol: Protocol::Original,
    extra_inputs: 0,
};

const NEO: Spec = Spec {
    name: "Stream Deck Neo",
    product_id: 0x009a,
    columns: 4,
    rows: 2,
    key_size: 96,
    image_format: ImageFormat::Jpeg,
    transform: mirrored(Rotation::None, true, true),
    protocol: Protocol::Gen2,
    extra_inputs: 2,
};

const ORIGINAL_V2: Spec = gen2_15("Stream Deck (v2)", 0x006d);
const MK2: Spec = gen2_15("Stream Deck MK.2", 0x0080);
const MK2_SCISSOR: Spec = gen2_15("Stream Deck MK.2 (scissor keys)", 0x00a5);
const MINI: Spec = mini("Stream Deck Mini", 0x0063);
const MINI_MK2: Spec = mini("Stream Deck Mini MK.2", 0x0090);
const XL: Spec = gen2_32("Stream Deck XL", 0x006c);
const XL_V2: Spec = gen2_32("Stream Deck XL (v2)", 0x008f);
const MODULE_6: Spec = mini("Stream Deck Module 6", 0x00b8);
const MODULE_15: Spec = gen2_15("Stream Deck Module 15", 0x00b9);
const MODULE_32: Spec = gen2_32("Stream Deck Module 32", 0x00ba);

impl Model {
    /// Every supported model.
    pub const ALL: [Model; 12] = [
        Model::Original,
        Model::OriginalV2,
        Model::Mk2,
        Model::Mk2Scissor,
        Model::Mini,
        Model::MiniMk2,
        Model::Xl,
        Model::XlV2,
        Model::Neo,
        Model::Module6,
        Model::Module15,
        Model::Module32,
    ];

    /// The model's hardware description.
    #[must_use]
    pub const fn spec(self) -> &'static Spec {
        match self {
            Model::Original => &ORIGINAL,
            Model::OriginalV2 => &ORIGINAL_V2,
            Model::Mk2 => &MK2,
            Model::Mk2Scissor => &MK2_SCISSOR,
            Model::Mini => &MINI,
            Model::MiniMk2 => &MINI_MK2,
            Model::Xl => &XL,
            Model::XlV2 => &XL_V2,
            Model::Neo => &NEO,
            Model::Module6 => &MODULE_6,
            Model::Module15 => &MODULE_15,
            Model::Module32 => &MODULE_32,
        }
    }

    /// Looks up a model by USB vendor and product ID.
    #[must_use]
    pub fn from_usb_id(vendor_id: u16, product_id: u16) -> Option<Model> {
        if vendor_id != ELGATO_VENDOR_ID {
            return None;
        }
        Model::ALL
            .into_iter()
            .find(|model| model.spec().product_id == product_id)
    }

    /// Short, stable identifier, used on the command line and in capture
    /// files (`mk2`, `xl-v2`, …).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Model::Original => "original",
            Model::OriginalV2 => "original-v2",
            Model::Mk2 => "mk2",
            Model::Mk2Scissor => "mk2-scissor",
            Model::Mini => "mini",
            Model::MiniMk2 => "mini-mk2",
            Model::Xl => "xl",
            Model::XlV2 => "xl-v2",
            Model::Neo => "neo",
            Model::Module6 => "module-6",
            Model::Module15 => "module-15",
            Model::Module32 => "module-32",
        }
    }

    /// Looks up a model by [`Model::id`].
    #[must_use]
    pub fn from_id(id: &str) -> Option<Model> {
        Model::ALL.into_iter().find(|model| model.id() == id)
    }

    /// Number of keys.
    #[must_use]
    pub const fn key_count(self) -> u8 {
        self.spec().key_count()
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.spec().name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_ids_are_unique() {
        for (i, a) in Model::ALL.iter().enumerate() {
            for b in &Model::ALL[i + 1..] {
                assert_ne!(a.spec().product_id, b.spec().product_id, "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn ids_round_trip() {
        for model in Model::ALL {
            assert_eq!(Model::from_id(model.id()), Some(model));
            assert_eq!(
                Model::from_usb_id(ELGATO_VENDOR_ID, model.spec().product_id),
                Some(model)
            );
        }
    }

    #[test]
    fn rejects_other_vendors_and_unknown_products() {
        assert_eq!(Model::from_usb_id(0x1234, 0x0060), None);
        // Stream Deck Pedal: not a v1 device.
        assert_eq!(Model::from_usb_id(ELGATO_VENDOR_ID, 0x0086), None);
    }

    #[test]
    fn key_counts() {
        let counts: Vec<u8> = Model::ALL.iter().map(|m| m.key_count()).collect();
        assert_eq!(counts, [15, 15, 15, 15, 6, 6, 32, 32, 8, 6, 15, 32]);
    }
}

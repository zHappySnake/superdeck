// SPDX-License-Identifier: GPL-3.0-or-later

//! The bundled default font and the font database.

use std::sync::Arc;

use resvg::usvg::fontdb::{Database, Source};

/// Family name of the bundled default font.
pub const DEFAULT_FONT_FAMILY: &str = "Noto Sans";

/// Noto Sans 2.015 (unhinted), from
/// <https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015>,
/// under the SIL Open Font License 1.1 (`fonts/OFL.txt`).
static BUNDLED: [&[u8]; 4] = [
    include_bytes!("../fonts/NotoSans-Regular.ttf"),
    include_bytes!("../fonts/NotoSans-Bold.ttf"),
    include_bytes!("../fonts/NotoSans-Italic.ttf"),
    include_bytes!("../fonts/NotoSans-BoldItalic.ttf"),
];

/// Builds the font database: the bundled fonts first, so they win over an
/// installed copy of Noto Sans, then the installed fonts if `system` is set.
pub(crate) fn database(system: bool) -> Database {
    let mut db = Database::new();
    for data in BUNDLED {
        db.load_font_source(Source::Binary(Arc::new(data)));
    }
    if system {
        db.load_system_fonts();
    }
    db.set_sans_serif_family(DEFAULT_FONT_FAMILY);
    db
}

//! Optional craft-fonts build input (see storytold/craft-fonts `docs/integration.md` and
//! craftrules `standards/fonts.md`).
//!
//! With `CRAFT_FONTS_DIR=<absolute path of a craft-fonts checkout>` this embeds the document
//! families below, whole, as [`CRAFT_FONTS`](crate::fonts::CRAFT_FONTS); unset, `CRAFT_FONTS` is
//! empty and nothing else changes. A `CRAFT_FONTS_DIR` that is not a checkout, or that lacks one
//! of the families, is a warning and an empty table, or an error with `CRAFT_FONTS_REQUIRED=1`
//! (release builds).
//!
//! Titles and captions fall back only to bundled and craft-fonts faces, never to installed fonts,
//! so each script captions should draw in the default font needs a family here. The web (wasm32)
//! build embeds only BIZ UDPGothic Regular (~4.5 MB) to stay under the web size budget
//! (Cloudflare serves files up to 25 MiB). Only the local directory is read: no network.

/// Gothic for sans text and bold, a Mincho for serif text, Simplified Chinese, then one face per
/// other script the captions may use (Noto Sans is the only Devanagari face).
const DOCUMENT_FAMILIES: &[&str] = &[
    "BIZ UDPGothic",
    "BIZ UDMincho",
    "Noto Sans CJK SC",
    "Noto Sans Arabic",
    "Noto Sans Thai",
    "Noto Sans Georgian",
    "Noto Sans Armenian",
    "Noto Sans Ethiopic",
    "Noto Sans",
];

/// The families `filmcraft-ui-egui` embeds in its interface table (the CJK interface cuts, the
/// emoji and symbol faces); the interface shares the non-CJK faces above.
const INTERFACE_FAMILIES: &[&str] = &[
    "Noto Sans SC UI",
    "Noto Sans TC UI",
    "Noto Sans HK UI",
    "Noto Sans KR UI",
    "Noto Color Emoji",
    "Noto Emoji",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    "Noto Sans Math",
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let wasm = craft_fonts_build::target_is_wasm();
    let written = craft_fonts_build::Embed::documents(DOCUMENT_FAMILIES)
        .filter(move |face| !wasm || (face.family == "BIZ UDPGothic" && face.style == "Regular"))
        .embedded_elsewhere(INTERFACE_FAMILIES)
        .write("craft_fonts.rs");
    if let Err(e) = written {
        println!("cargo::error={e}");
    }
}

//! Right-to-left scripts (Arabic, Hebrew) in the interface: reading order.
//!
//! Reading order: egui shapes each font run with harfrust, so a word's letters join and run right
//! to left, but it places the runs left to right in logical order with no bidi pass: in
//! "كلمة أخرى" the second word comes out first. [`Lines`] lays out document text (captions) in
//! reading order for display. Lines are broken on the logical text as egui wraps it, each line's
//! runs are put in visual order by the Unicode Bidirectional Algorithm (UAX #9, as the text engine
//! does for titles), and every right-to-left word becomes its own layout section, which egui's
//! shaper draws right to left. Text being edited keeps egui's logical order: `TextEdit` maps the
//! caret by character index, so a reordered galley would put it in the wrong place.

use std::ops::Range;
use std::sync::Arc;

use egui::epaint::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontId, Galley, Painter, Rect, Vec2, pos2};
use unicode_bidi::{BidiClass, BidiInfo, bidi_class};

/// Whether `text` has a right-to-left letter (Arabic, Hebrew…), the only text [`Lines`] reorders.
pub(crate) fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| matches!(bidi_class(c), BidiClass::R | BidiClass::AL))
}

/// Characters that stay together, in logical order, in a right-to-left word: letters, marks and
/// joiners. egui shapes such a word as one right-to-left run.
fn in_word(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL | BidiClass::NSM | BidiClass::BN)
}

/// A bracket shown right to left takes its mirrored glyph (UAX #9 rule L4).
fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

/// The layout sections of one line of `bidi.text` (byte range `line`, inside one paragraph), left
/// to right. A left-to-right run stays whole, except that Arabic-Indic digits get a section each
/// so the shaper cannot run them right to left. A right-to-left run is split into words
/// ([`in_word`]) and single other characters (mirrored), listed in reverse: egui shapes each word
/// right to left itself.
fn line_sections(bidi: &BidiInfo<'_>, line: Range<usize>) -> Vec<String> {
    let text = bidi.text;
    let Some(whole) = text.get(line.clone()).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let Some(para) = bidi.paragraphs.iter().find(|p| p.range.start <= line.start && line.end <= p.range.end) else {
        return vec![whole.to_owned()];
    };
    let (levels, runs) = bidi.visual_runs(para, line);
    let mut out = Vec::new();
    for run in runs {
        let Some(s) = text.get(run.clone()) else {
            continue;
        };
        if levels.get(run.start).is_some_and(|l| l.is_rtl()) {
            let mut units = Vec::new();
            let mut word = String::new();
            for c in s.chars() {
                if in_word(c) {
                    word.push(c);
                    continue;
                }
                if !word.is_empty() {
                    units.push(std::mem::take(&mut word));
                }
                units.push(mirror(c).to_string());
            }
            if !word.is_empty() {
                units.push(word);
            }
            out.extend(units.into_iter().rev());
        } else {
            let mut plain = String::new();
            for c in s.chars() {
                if bidi_class(c) == BidiClass::AN {
                    if !plain.is_empty() {
                        out.push(std::mem::take(&mut plain));
                    }
                    out.push(c.to_string());
                } else {
                    plain.push(c);
                }
            }
            if !plain.is_empty() {
                out.push(plain);
            }
        }
    }
    out
}

/// Byte ranges of the lines egui broke `text` into in `galley` (newlines left out), or None when
/// the rows do not account for every character.
fn line_ranges(text: &str, galley: &Galley) -> Option<Vec<Range<usize>>> {
    let bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    let mut out = Vec::with_capacity(galley.rows.len());
    let mut at = 0usize;
    for row in &galley.rows {
        let end = at.checked_add(row.glyphs.len())?;
        out.push(*bounds.get(at)?..*bounds.get(end)?);
        at = end.checked_add(usize::from(row.ends_with_newline))?;
    }
    (at == bounds.len().saturating_sub(1)).then_some(out)
}

/// Byte ranges of the lines of `text` between newlines.
fn paragraph_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            out.push(start..i);
            start = i + 1;
        }
    }
    out.push(start..text.len());
    out
}

/// Document text laid out for display in reading order, one galley per line.
pub(crate) struct Lines {
    rows: Vec<Arc<Galley>>,
    /// The text starts right to left (its first strong letter is Arabic or Hebrew): lines align right.
    pub rtl: bool,
    pub size: Vec2,
}

impl Lines {
    /// Lay out `text` in `font`, wrapped at `wrap_width` (`f32::INFINITY`: only at `\n`). Text
    /// without right-to-left letters is laid out as egui always does, in one galley.
    pub fn new(painter: &Painter, text: &str, font: FontId, color: Color32, wrap_width: f32) -> Self {
        let logical = painter.layout(text.to_owned(), font.clone(), color, wrap_width);
        if !has_rtl(text) {
            return Self { size: logical.size(), rows: vec![logical], rtl: false };
        }
        let bidi = BidiInfo::new(text, None);
        let rtl = bidi.paragraphs.first().is_some_and(|p| p.level.is_rtl());
        let lines = line_ranges(text, &logical).unwrap_or_else(|| paragraph_ranges(text));
        let rows: Vec<Arc<Galley>> = lines
            .into_iter()
            .map(|line| {
                let mut job = LayoutJob::default();
                for s in line_sections(&bidi, line) {
                    let start = job.text.len();
                    job.text.push_str(&s);
                    // pushed, not appended: `append` merges sections of equal format, and each
                    // right-to-left word must be shaped on its own
                    job.sections.push(LayoutSection {
                        leading_space: 0.0,
                        byte_range: ByteIndex(start)..ByteIndex(job.text.len()),
                        format: TextFormat::simple(font.clone(), color),
                    });
                }
                if job.sections.is_empty() { painter.layout_no_wrap(String::new(), font.clone(), color) } else { painter.layout_job(job) }
            })
            .collect();
        let size = rows.iter().fold(Vec2::ZERO, |s, g| Vec2::new(s.x.max(g.size().x), s.y + g.size().y));
        Self { rows, rtl, size }
    }

    /// Paint the lines down from the top of `rect`: from its left edge, or from its right edge when
    /// [`Self::rtl`].
    pub fn paint(&self, painter: &Painter, rect: Rect, color: Color32) {
        let mut y = rect.min.y;
        for g in &self.rows {
            let x = if self.rtl { rect.max.x - g.size().x } else { rect.min.x };
            painter.galley(pos2(x, y), g.clone(), color);
            y += g.size().y;
        }
    }
}

/// Paint one line of document text vertically centred in `rect`, in reading order: from the left
/// edge, or from the right edge when it reads right to left, so a clipped line still shows its start.
pub(crate) fn paint_line(painter: &Painter, rect: Rect, text: &str, font: FontId, color: Color32) {
    let lines = Lines::new(painter, text, font, color, f32::INFINITY);
    let top = rect.center().y - lines.size.y / 2.0;
    lines.paint(painter, Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, top + lines.size.y)), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sections of each line of `text` (split at newlines), left to right.
    fn visual(text: &str) -> Vec<Vec<String>> {
        let bidi = BidiInfo::new(text, None);
        paragraph_ranges(text).into_iter().map(|l| line_sections(&bidi, l)).collect()
    }

    #[test]
    fn rtl_words_are_listed_last_word_first() {
        assert_eq!(visual("كلمة أخرى"), [["أخرى", " ", "كلمة"]]);
        assert_eq!(visual("שלום עולם"), [["עולם", " ", "שלום"]]);
    }

    #[test]
    fn marks_and_joiners_stay_in_their_word() {
        // fatha, shadda and a zero-width non-joiner belong to the letters around them
        assert_eq!(visual("كَتَبَ مُحَمَّد"), [["مُحَمَّد", " ", "كَتَبَ"]]);
        assert_eq!(visual("می\u{200c}خواهم بود"), [["بود", " ", "می\u{200c}خواهم"]]);
    }

    #[test]
    fn latin_and_numbers_inside_rtl_keep_their_order() {
        assert_eq!(visual("قال FilmCraft 0.6 جيد"), [["جيد", " ", "FilmCraft 0.6", " ", "قال"]]);
        // Arabic-Indic digits read left to right, one section each so the shaper keeps them so
        assert_eq!(visual("سنة ١٤٤٥"), [["١", "٤", "٤", "٥", " ", "سنة"]]);
    }

    #[test]
    fn rtl_punctuation_and_brackets() {
        assert_eq!(visual("نعم، (نص)"), [["(", "نص", ")", " ", "،", "نعم"]]);
        assert_eq!(visual("لماذا؟"), [["لماذا؟"]]);
    }

    #[test]
    fn ltr_paragraph_with_an_rtl_phrase() {
        assert_eq!(visual("Lesson كلمة أخرى end"), [["Lesson ", "أخرى", " ", "كلمة", " end"]]);
        assert!(!has_rtl("Lesson 3: (draft)"));
        assert!(has_rtl("Lesson كلمة"));
    }

    #[test]
    fn each_line_is_reordered_on_its_own() {
        assert_eq!(visual("سطر أول\nسطر ثان"), [["أول", " ", "سطر"], ["ثان", " ", "سطر"]]);
        assert_eq!(visual("نص\n\nEnglish"), [vec!["نص"], vec![], vec!["English"]]);
    }

    #[test]
    fn hostile_ranges_give_nothing() {
        let text = "كلمة";
        let bidi = BidiInfo::new(text, None);
        assert!(line_sections(&bidi, 1..3).is_empty(), "not on a char boundary");
        assert!(line_sections(&bidi, 0..99).is_empty(), "past the end");
        #[allow(clippy::reversed_empty_ranges)]
        let backwards = 4..2;
        assert!(line_sections(&bidi, backwards).is_empty());
    }

    /// With an Arabic face installed, captions lay out with real glyphs in reading order: in
    /// "كلمة أخرى" the whole second word draws left of the first, its last letter leftmost.
    #[test]
    fn lines_draw_arabic_in_reading_order() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx, &crate::theme::Tokens::for_kind(crate::theme::ThemeKind::default()));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        if !ctx.fonts_mut(|f| f.has_glyphs(&FontId::proportional(13.0), "كلمة")) {
            eprintln!("SKIPPED: no craft-fonts or installed Arabic font");
            return;
        }
        let painter = Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
        let font = FontId::proportional(13.0);
        let lines = Lines::new(&painter, "كلمة أخرى", font.clone(), Color32::WHITE, f32::INFINITY);
        assert!(lines.rtl);
        let row = lines.rows.first().and_then(|g| g.rows.first()).map(|r| r.glyphs.clone()).unwrap_or_default();
        // egui shapes each word on its own (letters joined, in visual order) and pads the row with
        // empty glyphs so every char keeps a cursor position: only drawn glyphs count here
        let drawn: Vec<_> = row.iter().filter(|g| !g.uv_rect.is_nothing()).collect();
        let order = drawn.iter().map(|g| g.chr).collect::<String>();
        let xs = |word: &str| drawn.iter().filter(|g| word.contains(g.chr)).map(|g| g.pos.x).collect::<Vec<f32>>();
        let (first, second) = (xs("كلمة"), xs("أخرى"));
        assert!(!first.is_empty() && !second.is_empty(), "{order:?}");
        assert!(second.iter().fold(f32::MIN, |a, &b| a.max(b)) < first.iter().fold(f32::MAX, |a, &b| a.min(b)), "{order:?}");
        let leftmost = drawn.iter().min_by(|a, b| a.pos.x.total_cmp(&b.pos.x)).map(|g| g.chr);
        assert_eq!(leftmost, Some('ى'), "{order:?}");
        let missing = painter.layout_no_wrap("\u{fffd}".into(), font, Color32::WHITE);
        let missing_uv = missing.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.uv_rect);
        assert!(drawn.iter().all(|g| Some(g.uv_rect) != missing_uv), "an Arabic letter drew as a missing glyph");
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }

    /// Lines broken by wrapping are each reordered: the first line holds the first words.
    #[test]
    fn line_ranges_follow_the_rows() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let painter = Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
        let text = "one two three\nfour";
        let galley = painter.layout(text.to_owned(), FontId::proportional(13.0), Color32::WHITE, 40.0);
        let ranges = line_ranges(text, &galley).unwrap_or_default();
        assert!(ranges.len() >= 3, "{ranges:?}");
        let joined: Vec<&str> = ranges.iter().filter_map(|r| text.get(r.clone())).collect();
        assert_eq!(joined.concat().replace(' ', ""), "onetwothreefour");
        assert_eq!(ranges.last().and_then(|r| text.get(r.clone())), Some("four"));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }
}

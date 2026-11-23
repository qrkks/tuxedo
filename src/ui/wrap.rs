use std::ops::Range;

use unicode_linebreak::linebreaks;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn cell_width(text: &str) -> usize {
    text.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// Use Unicode line-break opportunities that fall on grapheme boundaries.
/// Measure each grapheme in terminal cells, as ratatui does when painting.
pub(super) fn unicode_word_parts(word: &str) -> impl Iterator<Item = (&str, usize)> {
    let mut breaks = linebreaks(word).peekable();
    let mut start = 0;
    let mut cells = 0;
    word.grapheme_indices(true)
        .filter_map(move |(index, grapheme)| {
            let end = index + grapheme.len();
            cells += grapheme.width();
            while breaks.next_if(|&(offset, _)| offset < end).is_some() {}
            breaks.next_if(|&(offset, _)| offset == end)?;
            let part = &word[start..end];
            start = end;
            Some((part, std::mem::take(&mut cells)))
        })
}

/// Wrap editable text into contiguous byte ranges, preserving the spaces
/// at row boundaries so every insertion point has a display position.
pub(super) fn draft_row_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut end = 0;
    let mut cells = 0;
    let mut push = |part: &str, part_width| {
        if cells + part_width > width && start < end {
            rows.push(start..end);
            start = end;
            cells = 0;
        }
        end += part.len();
        cells += part_width;
    };
    for (part, part_width) in unicode_word_parts(text) {
        if part_width <= width {
            push(part, part_width);
        } else {
            for grapheme in part.graphemes(true) {
                push(grapheme, grapheme.width());
            }
        }
    }
    rows.push(start..end);
    rows
}

/// The visible byte ranges for each wrapped line of a styled task body.
pub(super) fn row_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut row = 0..0;
    let mut used = 0;
    let mut push = |start, end, cells| {
        if !row.is_empty() {
            let gap = cell_width(&text[row.end..start]);
            if used + gap + cells <= width {
                row.end = end;
                used += gap + cells;
                return;
            }
            rows.push(std::mem::take(&mut row));
        }
        row = start..end;
        used = cells;
    };

    for word in text.split_whitespace() {
        let mut push_part = |part: &str, cells| {
            let start = part.as_ptr() as usize - text.as_ptr() as usize;
            if cells <= width {
                push(start, start + part.len(), cells);
            } else {
                for (offset, grapheme) in part.grapheme_indices(true) {
                    let begin = start + offset;
                    push(begin, begin + grapheme.len(), grapheme.width());
                }
            }
        };
        if word.is_ascii() || super::task_row::is_url_token(word) {
            push_part(word, cell_width(word));
        } else {
            for (part, cells) in unicode_word_parts(word) {
                push_part(part, cells);
            }
        }
    }

    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

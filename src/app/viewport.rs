use std::ops::Range;

#[derive(Default)]
pub(crate) struct TaskViewport {
    /// Display rows occupied by each task, indexed by the visible-task cursor.
    pub rows: Vec<Range<usize>>,
    offset: u16,
    height: u16,
    total: usize,
    page_cursor: Option<usize>,
}

impl TaskViewport {
    pub fn invalidate(&mut self) {
        self.rows.clear();
        self.follow_cursor();
    }

    pub fn follow_cursor(&mut self) {
        self.page_cursor = None;
    }

    pub fn render_offset(&mut self, cursor: usize, height: u16, total: usize) -> u16 {
        self.height = height;
        self.total = total;
        self.offset = self.offset.min(self.max_offset());
        let row = self.rows.get(cursor);
        let top = usize::from(self.offset);

        // A page can open in the middle of a wrapped task. Keep that
        // position while the selected task still intersects the viewport.
        if self.page_cursor != Some(cursor)
            || !row.is_some_and(|row| row.start < top + usize::from(height) && row.end > top)
        {
            self.offset = crate::ui::keep_cursor_visible(self.offset, row.cloned(), height, total);
            self.follow_cursor();
        }
        self.offset
    }

    pub fn page(&mut self, cursor: usize, forward: bool) -> Option<usize> {
        let height = usize::from(self.height);
        let current = self.rows.get(cursor)?;
        if height == 0 {
            return None;
        }
        let old_top = usize::from(self.offset);
        let top = if forward {
            old_top
                .saturating_add(height)
                .min(usize::from(self.max_offset()))
        } else {
            old_top.saturating_sub(height)
        };
        let last = self.rows.len() - 1;
        let cursor = if top == old_top {
            if forward { last } else { 0 }
        } else {
            let relative = current.start.saturating_sub(old_top).min(height - 1);
            let first_visible = self.rows.partition_point(|row| row.end <= top).min(last);
            let last_visible = self
                .rows
                .partition_point(|row| row.start < top + height)
                .saturating_sub(1)
                .max(first_visible);
            self.rows
                .partition_point(|row| row.end <= top + relative)
                .clamp(first_visible, last_visible)
        };

        self.offset = top as u16;
        self.page_cursor = Some(cursor);
        Some(cursor)
    }

    fn max_offset(&self) -> u16 {
        self.total
            .saturating_sub(usize::from(self.height))
            .min(usize::from(u16::MAX)) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::TaskViewport;

    #[test]
    fn paging_keeps_continuation_lines_visible_inside_a_long_task() {
        let mut view = TaskViewport {
            rows: vec![0..9, 9..10],
            ..TaskViewport::default()
        };

        assert_eq!(view.render_offset(0, 4, 10), 0);
        assert_eq!(view.page(0, true), Some(0));
        assert_eq!(view.render_offset(0, 4, 10), 4);
        assert_eq!(view.page(0, true), Some(0));
        assert_eq!(view.render_offset(0, 4, 10), 6);
        assert_eq!(view.page(0, true), Some(1));
        assert_eq!(view.render_offset(1, 4, 10), 6);
        assert_eq!(view.page(1, false), Some(0));
        assert_eq!(view.render_offset(0, 4, 10), 2);
        assert_eq!(view.page(0, false), Some(0));
        assert_eq!(view.render_offset(0, 4, 10), 0);
    }
}

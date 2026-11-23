use super::App;
use super::types::{Sort, View};
use crate::core::filter::{self, ListDueBucket};

/// One entry per visible row, parallel to `visible_cache`. Renderers detect
/// group transitions by comparing successive entries; under `Sort::File` every
/// row is `None` so the renderer skips headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKey {
    None,
    ArchiveDate(String),
    /// `Some('A'..='Z')` for a graded priority, `None` for unprioritized.
    ListPriority(Option<char>),
    ListDue(ListDueBucket),
}

impl App {
    /// Visible indices in display order. Resolve them with `view_task` for
    /// views that read from the archive or combine both sources.
    pub fn visible_indices(&self) -> &[usize] {
        &self.visible_cache
    }

    /// Group key per row, parallel to `visible_indices()`.
    pub fn visible_groups(&self) -> &[GroupKey] {
        &self.visible_groups
    }

    /// Recompute the cached visible-index list and parallel group keys. Call
    /// after any mutation that affects filter/sort/view/tasks/archive.
    pub fn recompute_visible(&mut self) {
        self.task_viewports[self.view.idx()].get_mut().invalidate();
        match self.view {
            View::List => self.rebuild_list_cache(),
            View::Archive | View::Completed => self.rebuild_completion_cache(),
        }
    }

    fn rebuild_list_cache(&mut self) {
        let tasks = self.store.tasks();
        let today = self.store.today();
        // Resolved once, not per task — a `due:` term can drive a
        // business-day walk in `threshold::shift`.
        let needle = (!self.filter.search.is_empty())
            .then(|| filter::resolve_needle(&self.filter.search, today));

        let mut idxs: Vec<usize> = (0..tasks.len())
            .filter(|&i| {
                filter::list_predicate(
                    &tasks[i],
                    self.prefs.show_done,
                    self.prefs.show_future,
                    today,
                    &self.filter,
                    needle.as_ref(),
                )
            })
            .collect();

        filter::sort_by_prefs(&mut idxs, tasks, self.prefs.sort);

        let week_start = &self.week_start;

        let groups: Vec<GroupKey> = match self.prefs.sort {
            Sort::File => vec![GroupKey::None; idxs.len()],
            Sort::Priority | Sort::PriorityReverseFile => idxs
                .iter()
                .map(|&i| GroupKey::ListPriority(tasks[i].priority))
                .collect(),
            Sort::Due => idxs
                .iter()
                .map(|&i| GroupKey::ListDue(filter::due_bucket(&tasks[i], today, week_start)))
                .collect(),
        };
        self.visible_groups = groups;
        self.visible_cache = idxs;
    }

    fn rebuild_completion_cache(&mut self) {
        let mut idxs: Vec<usize> = if self.view == View::Completed {
            self.tasks()
                .iter()
                .chain(self.archive().tasks())
                .enumerate()
                .filter_map(|(idx, task)| task.done.then_some(idx))
                .collect()
        } else {
            (0..self.archive().len()).collect()
        };
        let done_date = |idx| {
            self.view_task(idx)
                .and_then(|task| task.done_date.as_deref())
        };
        idxs.sort_by_key(|&idx| std::cmp::Reverse(done_date(idx)));
        self.visible_groups = idxs
            .iter()
            .map(|&idx| GroupKey::ArchiveDate(done_date(idx).unwrap_or("unknown").to_owned()))
            .collect();
        self.visible_cache = idxs;
    }

    pub fn cur_abs(&self) -> Option<usize> {
        self.visible_cache.get(self.cursor).copied()
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.visible_cache.len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    /// Move the cursor to wherever `abs` lives in the current visible list.
    /// Falls back to clamping if `abs` was filtered out.
    pub(super) fn follow_cursor(&mut self, abs: usize) {
        if let Some(pos) = self.visible_cache.iter().position(|&i| i == abs) {
            self.cursor = pos;
        } else {
            self.clamp_cursor();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::build_app;
    use crate::core::filter::ListDueBucket;

    #[test]
    fn search_matches_subsequence() {
        let mut app = build_app("2026-05-01 Call dentist\n2026-05-01 buy milk\n");
        app.filter.search = "cade".into();
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 1);
    }

    #[test]
    fn search_matches_body_not_dates() {
        let mut app = build_app("2026-05-01 buy milk\n2026-04-01 something else\n");
        app.filter.search = "2026".into();
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 0);
    }

    #[test]
    fn search_due_range_matches_by_due_field_not_literal_text() {
        let mut app =
            build_app("in range due:2026-05-10\nout of range due:2026-05-20\nno due date here\n");
        app.filter.search = "due:+1w".into();
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 1);
    }

    #[test]
    fn visible_cache_updates_after_mutation() {
        let mut app = build_app("a\nb\nc\n");
        assert_eq!(app.visible_indices().len(), 3);
        app.draft_set("d".into());
        app.add_from_draft();
        assert_eq!(app.visible_indices().len(), 4);
    }

    #[test]
    fn list_cursor_survives_archive_roundtrip() {
        let mut app = build_app("a\nb\nc\nd\ne\n");
        app.cursor = 3;
        app.set_view(View::Archive);
        app.set_view(View::List);
        assert_eq!(app.cursor, 3, "cursor lost on List → Archive → List");
    }

    #[test]
    fn completed_view_combines_both_files_by_completion_date() {
        let mut app = build_app("pending\nx 2026-05-02 2026-05-01 live\nx undated\n");
        let path = app.archive().path().to_path_buf();
        app.store.archive = crate::app::Archive::for_test(
            crate::todo::parse_file(
                "x 2026-05-03 2026-04-01 archived\nx 2026-05-02 2026-04-01 same day\n",
            ),
            String::new(),
            path,
        );

        app.set_view(View::Completed);
        assert_eq!(
            app.visible_groups(),
            [
                GroupKey::ArchiveDate("2026-05-03".into()),
                GroupKey::ArchiveDate("2026-05-02".into()),
                GroupKey::ArchiveDate("2026-05-02".into()),
                GroupKey::ArchiveDate("unknown".into()),
            ]
        );
        for (cursor, body) in ["archived", "live", "same day", "undated"]
            .into_iter()
            .enumerate()
        {
            app.cursor = cursor;
            assert!(app.cur_task().is_some_and(|task| task.raw.ends_with(body)));
            assert_eq!(app.cur_task_index_in_tasks(), None);
        }

        app.set_view(View::Archive);
        assert_eq!(app.visible_indices().len(), 2);
    }

    #[test]
    fn archive_indices_point_into_archive_tasks() {
        let mut app = build_app("a\n");
        let path = app.archive().path().to_path_buf();
        app.store.archive = crate::app::Archive::for_test(
            crate::todo::parse_file(
                "x 2026-05-01 2026-04-01 first\nx 2026-05-02 2026-04-02 second\n",
            ),
            String::new(),
            path,
        );
        app.set_view(View::Archive);
        let idxs = app.visible_indices();
        assert_eq!(idxs.len(), 2);
        for &i in idxs {
            assert!(app.archive().tasks().get(i).is_some());
        }
    }

    #[test]
    fn list_groups_are_none_under_sort_file() {
        let mut app = build_app("(A) a\n(B) b\nc\n");
        app.prefs.sort = Sort::File;
        app.recompute_visible();
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 3);
        for g in groups {
            assert!(matches!(g, GroupKey::None));
        }
    }

    #[test]
    fn list_groups_track_priority_under_sort_priority() {
        let mut app = build_app("(A) a\n(B) b\nc\n(A) a2\n");
        app.prefs.sort = Sort::Priority;
        app.recompute_visible();
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[0], GroupKey::ListPriority(Some('A')));
        assert_eq!(groups[1], GroupKey::ListPriority(Some('A')));
        assert_eq!(groups[2], GroupKey::ListPriority(Some('B')));
        assert_eq!(groups[3], GroupKey::ListPriority(None));
    }

    #[test]
    fn list_groups_bucket_due_dates_under_sort_due() {
        let raw = "a due:2026-05-04\n\
                   b due:2026-05-06\n\
                   c due:2026-05-08\n\
                   d due:2026-05-15\n\
                   e due:2026-05-25\n\
                   f\n";
        let mut app = build_app(raw);
        app.prefs.sort = Sort::Due;
        app.recompute_visible();
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 6);
        assert_eq!(groups[0], GroupKey::ListDue(ListDueBucket::Overdue));
        assert_eq!(groups[1], GroupKey::ListDue(ListDueBucket::Today));
        assert_eq!(groups[2], GroupKey::ListDue(ListDueBucket::ThisWeek));
        assert_eq!(groups[3], GroupKey::ListDue(ListDueBucket::NextWeek));
        assert_eq!(groups[4], GroupKey::ListDue(ListDueBucket::Later));
        assert_eq!(groups[5], GroupKey::ListDue(ListDueBucket::NoDue));
    }

    #[test]
    fn future_absolute_threshold_hidden_by_default() {
        let mut app = build_app("future task t:2030-01-01\nvisible task\n");
        assert_eq!(app.visible_indices().len(), 1);
        assert_eq!(app.tasks()[app.visible_indices()[0]].raw, "visible task");
        app.prefs.show_future = true;
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 2);
    }

    #[test]
    fn relative_threshold_anchors_on_due() {
        let mut app = build_app("Pay rent due:2026-05-15 t:-3d\n");
        assert_eq!(app.visible_indices().len(), 0);
        app.prefs.show_future = true;
        app.recompute_visible();
        assert_eq!(app.visible_indices().len(), 1);
    }

    #[test]
    fn refresh_today_unhides_tasks_when_date_advances() {
        let mut app = build_app("future task t:2026-05-07\nvisible task\n");
        assert_eq!(app.visible_indices().len(), 1);
        let changed = app.refresh_today("2026-05-07".into());
        assert!(changed);
        assert_eq!(app.today(), "2026-05-07");
        assert_eq!(app.visible_indices().len(), 2);
    }

    #[test]
    fn refresh_today_is_noop_when_date_unchanged() {
        let mut app = build_app("a\n");
        let changed = app.refresh_today("2026-05-06".into());
        assert!(!changed);
        assert_eq!(app.today(), "2026-05-06");
    }

    #[test]
    fn archive_visible_groups_are_done_date_desc() {
        let mut app = build_app("a\n");
        let path = app.archive().path().to_path_buf();
        app.store.archive = crate::app::Archive::for_test(
            crate::todo::parse_file(
                "x 2026-04-01 2026-03-01 older\nx 2026-05-02 2026-04-02 newer\n",
            ),
            String::new(),
            path,
        );
        app.set_view(View::Archive);
        let groups = app.visible_groups();
        assert_eq!(groups.len(), 2);
        let first = match &groups[0] {
            GroupKey::ArchiveDate(d) => d.as_str(),
            _ => panic!("expected ArchiveDate"),
        };
        let second = match &groups[1] {
            GroupKey::ArchiveDate(d) => d.as_str(),
            _ => panic!("expected ArchiveDate"),
        };
        assert_eq!(first, "2026-05-02");
        assert_eq!(second, "2026-04-01");
    }
}

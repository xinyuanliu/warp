use std::path::PathBuf;

use warp_editor::model::CoreEditorModel;
use warp_editor::render::model::{
    BlockItem, HitTestOptions, LineCount, Location, RenderLineLocation,
};
use warpui::units::Pixels;
use warpui::{AppContext, ViewContext, ViewHandle};

use super::{CodeReviewView, CodeReviewViewState, FILE_HEADER_HEIGHT};
use crate::code::buffer_location::LocalOrRemotePath;
use crate::code::editor::line::EditorLineLocation;
use crate::code::editor::view::CodeEditorView;
use crate::code_review::comments::{AttachedReviewCommentTarget, CommentId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeReviewVisibleAnchorForTest {
    pub file_path: String,
    pub line_number: usize,
    pub line_text: String,
}

impl CodeReviewView {
    pub fn visible_anchor_for_test(
        &self,
        ctx: &AppContext,
    ) -> Option<CodeReviewVisibleAnchorForTest> {
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return None;
        };

        let file_index = self.viewported_list_state.get_scroll_index();
        let (_, file_state) = state.file_states.get_index(file_index)?;
        let editor_state = file_state.editor_state.as_ref()?;
        let scroll_offset = self.viewported_list_state.get_scroll_offset();
        let content_y = (scroll_offset - Pixels::new(FILE_HEADER_HEIGHT) + Pixels::new(2.0))
            .max(Pixels::zero());

        let editor = editor_state.editor.as_ref(ctx).editor();
        let render_state_handle = editor.as_ref(ctx).model.as_ref(ctx).render_state().clone();
        let location = render_state_handle
            .as_ref(ctx)
            .render_coordinates_to_location(
                Pixels::new(10.0),
                content_y,
                &HitTestOptions {
                    force_text_selection: true,
                },
            );
        let char_offset = match location {
            Location::Text { char_offset, .. } => char_offset,
            Location::Block { start_offset, .. } => start_offset,
        };
        let render_state = render_state_handle.as_ref(ctx);
        let line_number = render_state.offset_to_softwrap_point(char_offset).row() as usize + 1;
        let (start_offset, end_offset) =
            render_state.line_number_to_offset_range(LineCount::from(line_number));
        let line_text = editor
            .as_ref(ctx)
            .model
            .as_ref(ctx)
            .content()
            .as_ref(ctx)
            .text_in_range(start_offset..end_offset)
            .into_string();

        Some(CodeReviewVisibleAnchorForTest {
            file_path: file_state.file_diff.file_path.clone(),
            line_number,
            line_text: line_text.trim_matches('\n').to_string(),
        })
    }

    pub fn scroll_to_line_for_test(
        &mut self,
        path: &str,
        line_number: usize,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return false;
        };

        let Some(editor_index) = state
            .file_states
            .iter()
            .position(|(_, file_state)| file_state.file_diff.file_path == path)
        else {
            return false;
        };
        let Some(editor_state) = state
            .file_states
            .get_index(editor_index)
            .and_then(|(_, file_state)| file_state.editor_state.as_ref())
        else {
            return false;
        };

        let editor = editor_state.editor().clone();
        let line_number = LineCount::from(line_number);
        let line = EditorLineLocation::Current {
            line_number,
            line_range: line_number..line_number + LineCount::from(1),
        };
        let (start_offset, end_offset) = editor
            .as_ref(ctx)
            .editor()
            .read(ctx, |code_editor_view, ctx| {
                code_editor_view.line_location_to_offsets(&line, ctx)
            });

        if let Some((start_top_y, _end_bottom_y)) =
            self.get_match_character_bounds(editor_index, start_offset, end_offset, ctx)
        {
            self.viewported_list_state
                .scroll_to_with_offset(editor_index, Pixels::new(FILE_HEADER_HEIGHT) + start_top_y);
            self.horizontally_scroll_to_match(editor_index, start_offset, end_offset, ctx);

            // Eagerly compute and store scroll context so it is available
            // before the next buffer edit (the debounce may not have fired yet).
            let context = self.compute_scroll_context_for_index(editor_index, &editor, ctx);
            if let Some(context) = context {
                self.viewported_list_state.set_scroll_context(Some(context));
            }

            ctx.notify();
            true
        } else {
            self.scroll_to_position(editor_index, start_offset, end_offset, 0.0, ctx);
            ctx.notify();
            false
        }
    }

    /// Scrolls the code review to the header region of the given file.
    /// The header region is the area above the editor content (< FILE_HEADER_HEIGHT).
    pub fn scroll_to_header_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return false;
        };

        let Some(editor_index) = state
            .file_states
            .iter()
            .position(|(_, file_state)| file_state.file_diff.file_path == path)
        else {
            return false;
        };
        let Some(editor_state) = state
            .file_states
            .get_index(editor_index)
            .and_then(|(_, file_state)| file_state.editor_state.as_ref())
        else {
            return false;
        };

        let editor = editor_state.editor().clone();

        // Scroll to 10px into the header (FILE_HEADER_HEIGHT is 41px)
        self.viewported_list_state
            .scroll_to_with_offset(editor_index, Pixels::new(10.0));

        let context = self.compute_scroll_context_for_index(editor_index, &editor, ctx);
        if let Some(context) = context {
            self.viewported_list_state.set_scroll_context(Some(context));
        }

        ctx.notify();
        true
    }

    /// Scrolls the code review past the end of editor content into the footer region.
    pub fn scroll_to_footer_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return false;
        };

        let Some(editor_index) = state
            .file_states
            .iter()
            .position(|(_, file_state)| file_state.file_diff.file_path == path)
        else {
            return false;
        };
        let Some(editor_state) = state
            .file_states
            .get_index(editor_index)
            .and_then(|(_, file_state)| file_state.editor_state.as_ref())
        else {
            return false;
        };

        let editor = editor_state.editor().clone();

        let content_height = editor
            .as_ref(ctx)
            .editor()
            .as_ref(ctx)
            .model
            .as_ref(ctx)
            .render_state()
            .as_ref(ctx)
            .height();

        // Scroll 5px past the editor content into the footer/margin area
        self.viewported_list_state.scroll_to_with_offset(
            editor_index,
            Pixels::new(FILE_HEADER_HEIGHT) + content_height + Pixels::new(5.0),
        );

        let context = self.compute_scroll_context_for_index(editor_index, &editor, ctx);
        if let Some(context) = context {
            self.viewported_list_state.set_scroll_context(Some(context));
        }

        ctx.notify();
        true
    }

    /// Scrolls the code review to a deleted (temporary) block near the given current buffer line.
    /// Scans forward from the y-offset of `near_line` to find the first TemporaryBlock.
    pub fn scroll_to_deleted_range_for_test(
        &mut self,
        path: &str,
        near_line: usize,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return false;
        };

        let Some(editor_index) = state
            .file_states
            .iter()
            .position(|(_, file_state)| file_state.file_diff.file_path == path)
        else {
            return false;
        };
        let Some(editor_state) = state
            .file_states
            .get_index(editor_index)
            .and_then(|(_, file_state)| file_state.editor_state.as_ref())
        else {
            return false;
        };

        let editor = editor_state.editor().clone();
        let editor_model_handle = editor.as_ref(ctx).editor().as_ref(ctx).model.clone();

        // Phase 1: Find the y-offset of a temporary block near the given line.
        let found_offset = {
            let editor_model = editor_model_handle.as_ref(ctx);
            let render_state = editor_model.render_state().as_ref(ctx);

            // Get approximate content-relative y position of near_line.
            // vertical_offset_at_render_location internally borrows and releases
            // the content RefCell, so calling content() afterwards is safe.
            let line_offset = render_state
                .vertical_offset_at_render_location(RenderLineLocation::Current(LineCount::from(
                    near_line,
                )))
                .unwrap_or(Pixels::zero());

            let content = render_state.content();
            let mut y = line_offset.as_f32() as f64;
            let scan_limit = y + 2000.0;
            let mut found = None;

            while y < scan_limit {
                let Some(block) = content.block_at_height(y) else {
                    break;
                };
                if matches!(block.item, BlockItem::TemporaryBlock { .. }) {
                    found = Some(block.start_y_offset + Pixels::new(5.0));
                    break;
                }
                // Advance past this block
                let block_end = (block.start_y_offset + block.item.height()).as_f32() as f64;
                y = if block_end <= y {
                    y + 1.0
                } else {
                    block_end + 0.5
                };
            }

            found
        };

        let Some(offset_in_editor) = found_offset else {
            return false;
        };

        self.viewported_list_state.scroll_to_with_offset(
            editor_index,
            Pixels::new(FILE_HEADER_HEIGHT) + offset_in_editor,
        );

        let context = self.compute_scroll_context_for_index(editor_index, &editor, ctx);
        if let Some(context) = context {
            self.viewported_list_state.set_scroll_context(Some(context));
        }

        ctx.notify();
        true
    }

    /// Returns a string describing which scroll region the current scroll position
    /// is in: "header", "current_line", "removed_line", "footer", or "unknown".
    pub fn scroll_region_for_test(&self, ctx: &AppContext) -> String {
        let file_index = self.viewported_list_state.get_scroll_index();
        let scroll_offset = self.viewported_list_state.get_scroll_offset();
        let file_header_height = Pixels::new(FILE_HEADER_HEIGHT);

        if scroll_offset < file_header_height {
            return "header".to_string();
        }

        let CodeReviewViewState::Loaded(state) = self.state() else {
            return "unknown".to_string();
        };

        let Some((_, file_state)) = state.file_states.get_index(file_index) else {
            return "unknown".to_string();
        };

        let Some(editor_state) = &file_state.editor_state else {
            return "unknown".to_string();
        };

        let editor_model = editor_state
            .editor
            .as_ref(ctx)
            .editor()
            .as_ref(ctx)
            .model
            .as_ref(ctx);
        let render_state = editor_model.render_state().as_ref(ctx);
        let content_height = render_state.height();
        let scroll_in_editor = scroll_offset - file_header_height;

        if scroll_in_editor >= content_height {
            return "footer".to_string();
        }

        let content = render_state.content();
        if let Some(block) = content.block_at_height(scroll_in_editor.as_f32() as f64) {
            match block.item {
                BlockItem::TemporaryBlock { .. } => return "removed_line".to_string(),
                _ => return "current_line".to_string(),
            }
        }

        "unknown".to_string()
    }

    pub fn all_editors_loaded_for_test(&self) -> bool {
        self.all_editors_loaded()
    }

    pub fn line_text_for_test(
        &self,
        path: &str,
        line_number: usize,
        ctx: &AppContext,
    ) -> Option<String> {
        // Test helper: probe by both the raw path (wrapped as a local
        // `LocalOrRemotePath`) and by the repo-joined absolute path.
        let local_path = LocalOrRemotePath::Local(PathBuf::from(path));
        let editor = if let Some(editor) = self.editor_for_path(&local_path, ctx) {
            editor
        } else {
            let absolute_path = self.repo_path()?.join(path);
            self.editor_for_path(&absolute_path, ctx)?
        };
        let text = editor
            .as_ref(ctx)
            .editor()
            .as_ref(ctx)
            .text(ctx)
            .into_string();
        let line_index = line_number.checked_sub(1)?;
        text.lines().nth(line_index).map(ToOwned::to_owned)
    }

    /// Resolve the inner [`CodeEditorView`] for a file path, probing both the raw path and the
    /// repo-joined absolute path (mirrors [`Self::line_text_for_test`]).
    fn code_editor_for_test(
        &self,
        path: &str,
        ctx: &AppContext,
    ) -> Option<ViewHandle<CodeEditorView>> {
        let local_path = LocalOrRemotePath::Local(PathBuf::from(path));
        let editor = if let Some(editor) = self.editor_for_path(&local_path, ctx) {
            editor
        } else {
            let absolute_path = self.repo_path()?.join(path);
            self.editor_for_path(&absolute_path, ctx)?
        };
        Some(editor.as_ref(ctx).editor().clone())
    }

    /// The 1-based line the inline composer is open at for `path`, or `None` if no composer is open.
    pub fn composer_open_line_for_test(&self, path: &str, ctx: &AppContext) -> Option<usize> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .composer_open_line_for_test(ctx)
    }

    /// Whether the inline composer is open for `path`.
    pub fn composer_open_for_test(&self, path: &str, ctx: &AppContext) -> bool {
        self.composer_open_line_for_test(path, ctx).is_some()
    }

    /// The current draft/body text of the active composer for `path`.
    pub fn composer_body_for_test(&self, path: &str, ctx: &AppContext) -> Option<String> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_body_for_test(ctx),
        )
    }

    /// Whether the composer's primary ("Comment"/"Update") button is disabled (empty draft).
    pub fn composer_save_disabled_for_test(&self, path: &str, ctx: &AppContext) -> Option<bool> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_save_disabled_for_test(ctx),
        )
    }

    /// The label of the composer's primary button ("Comment" for new, "Update" when editing).
    pub fn composer_primary_label_for_test(&self, path: &str, ctx: &AppContext) -> Option<String> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_primary_label_for_test(ctx),
        )
    }

    /// Whether the composer shows the "Remove" button (editing an existing comment).
    pub fn composer_show_remove_for_test(&self, path: &str, ctx: &AppContext) -> Option<bool> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_show_remove_for_test(ctx),
        )
    }

    /// Number of inline comment blocks in the per-view render state for `path`.
    pub fn inline_comment_block_count_for_test(
        &self,
        path: &str,
        ctx: &AppContext,
    ) -> Option<usize> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .inline_comment_block_count_for_test(ctx),
        )
    }

    /// On-screen (viewport-space) Y of the top of the given 1-based current line in `path`.
    pub fn line_viewport_y_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<f32> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .line_viewport_y_for_test(line, ctx)
    }

    /// On-screen (viewport-space) Y of the top of the inline comment block anchored at the given
    /// 1-based current line in `path`.
    pub fn comment_block_viewport_y_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<f32> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .comment_block_viewport_y_for_test(line, ctx)
    }

    /// Reserved height of the inline comment block anchored at the given 1-based current line in
    /// `path`.
    pub fn comment_block_height_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<f32> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .comment_block_height_for_test(line, ctx)
    }

    // --- Outer-list scroll observability ------------------------------------------------------
    //
    // Code-review scroll moves the OUTER viewported list, not the inner editor
    // `RenderState.scroll_top` (which stays at 0). Inner viewport-Y getters are therefore
    // scroll-independent, so on-screen visibility must be derived from the outer list's scroll
    // offset + viewport. These getters expose that observability so tests can assert a line or an
    // inline card is actually within the viewport after a scroll/jump.

    /// The outer code-review list's scroll offset, in pixels, within its scrolled-to file item
    /// (file-header + editor-content space).
    pub fn code_review_scroll_offset_for_test(&self) -> f32 {
        self.viewported_list_state.get_scroll_offset().as_f32()
    }

    /// The outer code-review list's scrolled-to file index.
    pub fn code_review_scroll_index_for_test(&self) -> usize {
        self.viewported_list_state.get_scroll_index()
    }

    /// The outer code-review list's viewport height, in pixels.
    pub fn code_review_viewport_height_for_test(&self) -> f32 {
        self.viewported_list_state.get_viewport_height().as_f32()
    }

    /// The outer-list item index of the file at `path`, matched the same way scrolling does
    /// (against `file_diff.file_path`).
    fn editor_list_index_for_test(&self, path: &str) -> Option<usize> {
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return None;
        };
        state
            .file_states
            .iter()
            .position(|(_, file_state)| file_state.file_diff.file_path == path)
    }

    /// Whether the editor-content-space range `[top_content_y, bottom_content_y]` for the file at
    /// list `editor_index` falls within the outer list's visible viewport (accounting for the file
    /// header above the editor content).
    fn is_editor_content_range_in_viewport(
        &self,
        editor_index: usize,
        top_content_y: f32,
        bottom_content_y: f32,
    ) -> bool {
        self.viewported_list_state.is_vertical_range_visible(
            editor_index,
            Pixels::new(FILE_HEADER_HEIGHT + top_content_y),
            Pixels::new(FILE_HEADER_HEIGHT + bottom_content_y),
        )
    }

    /// Whether the 1-based current `line` of `path` is within the outer list's visible viewport.
    pub fn is_line_in_viewport_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<bool> {
        let editor_index = self.editor_list_index_for_test(path)?;
        let editor = self.code_editor_for_test(path, ctx)?;
        let top = editor.as_ref(ctx).line_viewport_y_for_test(line, ctx)?;
        let line_height = editor.as_ref(ctx).base_line_height_for_test(ctx);
        Some(self.is_editor_content_range_in_viewport(editor_index, top, top + line_height))
    }

    /// Whether the WHOLE inline comment card anchored at the 1-based current `line` of `path` is
    /// within the outer list's visible viewport (top and bottom both visible).
    pub fn is_inline_card_in_viewport_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<bool> {
        let editor_index = self.editor_list_index_for_test(path)?;
        let editor = self.code_editor_for_test(path, ctx)?;
        let top = editor
            .as_ref(ctx)
            .comment_block_content_top_for_test(line, ctx)?;
        let height = editor
            .as_ref(ctx)
            .comment_block_height_for_test(line, ctx)?;
        Some(self.is_editor_content_range_in_viewport(editor_index, top, top + height))
    }

    /// Whether the TOP edge of the inline card anchored at the 1-based current `line` of `path` is
    /// within the outer list's visible viewport. Useful for cards taller than the viewport, whose
    /// top and bottom cannot be visible simultaneously.
    pub fn is_inline_card_top_in_viewport_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<bool> {
        let editor_index = self.editor_list_index_for_test(path)?;
        let editor = self.code_editor_for_test(path, ctx)?;
        let top = editor
            .as_ref(ctx)
            .comment_block_content_top_for_test(line, ctx)?;
        Some(self.is_editor_content_range_in_viewport(editor_index, top, top))
    }

    /// Whether the BOTTOM edge of the inline card anchored at the 1-based current `line` of `path`
    /// is within the outer list's visible viewport.
    pub fn is_inline_card_bottom_in_viewport_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<bool> {
        let editor_index = self.editor_list_index_for_test(path)?;
        let editor = self.code_editor_for_test(path, ctx)?;
        let top = editor
            .as_ref(ctx)
            .comment_block_content_top_for_test(line, ctx)?;
        let height = editor
            .as_ref(ctx)
            .comment_block_height_for_test(line, ctx)?;
        Some(self.is_editor_content_range_in_viewport(editor_index, top + height, top + height))
    }

    /// Content-space top offset of the inline card anchored at the 1-based current `line` of
    /// `path` (scroll-independent), or `None` if no card is anchored there.
    pub fn inline_card_content_top_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<f32> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .comment_block_content_top_for_test(line, ctx)
    }

    /// Scroll the outer code-review list so the given editor-content-space `content_y` for `path`
    /// sits at the top of the viewport (mirrors how `scroll_to_line_for_test` positions content,
    /// and persists the scroll context so it survives the next layout). Returns false if the file
    /// editor isn't available.
    pub fn scroll_editor_to_content_y_for_test(
        &mut self,
        path: &str,
        content_y: f32,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(editor_index) = self.editor_list_index_for_test(path) else {
            return false;
        };
        let CodeReviewViewState::Loaded(state) = self.state() else {
            return false;
        };
        let Some(editor) = state
            .file_states
            .get_index(editor_index)
            .and_then(|(_, file_state)| file_state.editor_state.as_ref())
            .map(|editor_state| editor_state.editor().clone())
        else {
            return false;
        };
        self.viewported_list_state
            .scroll_to_with_offset(editor_index, Pixels::new(FILE_HEADER_HEIGHT + content_y));
        if let Some(context) = self.compute_scroll_context_for_index(editor_index, &editor, ctx) {
            self.viewported_list_state.set_scroll_context(Some(context));
        }
        ctx.notify();
        true
    }

    /// Mark the first saved line comment outdated (mirrors a relocation/refresh flagging it stale),
    /// keeping it in the batch (so the bottom panel still shows it) while it drops out of the inline
    /// (editor) set. Returns the affected comment id, or `None` if no saved line comment exists.
    pub fn mark_first_line_comment_outdated_for_test(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) -> Option<CommentId> {
        let (comment_id, _) = self.first_line_comment_for_test(ctx)?;
        let model = self.active_comment_model.as_ref()?.clone();
        let mut comment = model.read(ctx, |batch, _| {
            batch.comments.iter().find(|c| c.id == comment_id).cloned()
        })?;
        comment.outdated = true;
        model.update(ctx, |batch, ctx| batch.upsert_comment(comment, ctx));
        Some(comment_id)
    }

    /// Whether the general/diffset (header-anchored) comment composer overlay is currently open.
    pub fn general_composer_overlay_present_for_test(&self) -> bool {
        self.comment_composer.is_some()
    }

    /// Open the general/diffset (header-anchored) comment composer overlay.
    pub fn open_general_composer_for_test(&mut self, ctx: &mut ViewContext<Self>) {
        self.open_review_comment_composer(None, ctx);
    }

    /// Open the inline composer on the given 1-based current line of `path`. Returns false if the
    /// editor isn't available.
    pub fn open_comment_line_for_test(
        &mut self,
        path: &str,
        line: usize,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.open_comment_line_for_test(line, ctx);
        });
        true
    }

    /// Type `text` into the focused composer for `path`.
    pub fn type_into_composer_for_test(
        &mut self,
        path: &str,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.type_into_composer_for_test(text, ctx);
        });
        true
    }

    /// Invoke the composer's primary save action for `path` (equivalent to clicking the button).
    pub fn save_composer_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.save_composer_for_test(ctx);
        });
        true
    }

    /// Cancel the composer for `path` (equivalent to clicking "Cancel").
    pub fn cancel_composer_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.cancel_composer_for_test(ctx);
        });
        true
    }

    /// Whether the composer's inner text editor for `path` currently holds focus.
    pub fn composer_inner_focused_for_test(&self, path: &str, ctx: &AppContext) -> Option<bool> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_inner_focused_for_test(ctx),
        )
    }

    /// Focus the composer's inner text editor for `path`.
    pub fn focus_composer_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.focus_composer_for_test(ctx);
        });
        true
    }

    /// Save the composer for `path` via the Cmd/Ctrl+Enter path.
    pub fn save_composer_via_cmd_enter_for_test(
        &mut self,
        path: &str,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.save_composer_via_cmd_enter_for_test(ctx);
        });
        true
    }

    /// Press Escape in the composer for `path` (closes only when the draft is empty).
    pub fn escape_composer_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.escape_composer_for_test(ctx);
        });
        true
    }

    /// Remove the comment currently being edited in the composer for `path`.
    pub fn remove_comment_for_test(&mut self, path: &str, ctx: &mut ViewContext<Self>) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.remove_comment_for_test(ctx);
        });
        true
    }

    /// The `(id, 1-based line)` of the first line-targeted saved comment in the active batch.
    fn first_line_comment_for_test(&self, ctx: &AppContext) -> Option<(CommentId, usize)> {
        self.active_comment_model.as_ref().and_then(|model| {
            model.read(ctx, |batch, _| {
                batch
                    .comments
                    .iter()
                    .find_map(|comment| match &comment.target {
                        AttachedReviewCommentTarget::Line { line, .. } => {
                            Some((comment.id, line.line_number()?.as_u32() as usize))
                        }
                        _ => None,
                    })
            })
        })
    }

    /// Number of saved comments in the active batch (all targets).
    pub fn saved_comment_count_for_test(&self, ctx: &AppContext) -> usize {
        self.active_comment_model
            .as_ref()
            .map(|model| model.read(ctx, |batch, _| batch.comments.len()))
            .unwrap_or(0)
    }

    /// Reopen the first saved line comment as a prefilled inline editor (mirrors the panel "Edit" /
    /// `RequestOpenSavedComment` path). Returns the 1-based line it was reopened at, or `None` if
    /// no saved line comment exists.
    pub fn reopen_saved_comment_for_test(&mut self, ctx: &mut ViewContext<Self>) -> Option<usize> {
        let (comment_id, line) = self.first_line_comment_for_test(ctx)?;
        self.handle_edit_comment(&comment_id, ctx);
        Some(line)
    }

    /// Jump to the first saved line comment via the panel "jump to comment" path
    /// ([`Self::handle_jump_to_comment_location`]), scrolling its inline card into view. Returns the
    /// 1-based line jumped to, or `None` if no saved line comment exists.
    pub fn jump_to_first_comment_for_test(&mut self, ctx: &mut ViewContext<Self>) -> Option<usize> {
        let (comment_id, line) = self.first_line_comment_for_test(ctx)?;
        self.handle_jump_to_comment_location(&comment_id, ctx);
        Some(line)
    }

    /// The rendered body text of the inline comment block anchored at the given 1-based current
    /// line of `path`, resolved through the block's hosted child.
    pub fn inline_comment_block_body_for_test(
        &self,
        path: &str,
        line: usize,
        ctx: &AppContext,
    ) -> Option<String> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .inline_comment_block_body_for_test(line, ctx)
    }

    /// Replace the active composer's draft body for `path` (mirrors deleting/retyping lines).
    pub fn set_composer_body_for_test(
        &mut self,
        path: &str,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(editor) = self.code_editor_for_test(path, ctx) else {
            return false;
        };
        editor.update(ctx, |editor, ctx| {
            editor.set_composer_body_for_test(text, ctx);
        });
        true
    }

    /// The active composer's inner content height for `path` (independent of the 200px cap).
    pub fn composer_inner_content_height_for_test(
        &self,
        path: &str,
        ctx: &AppContext,
    ) -> Option<f32> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_inner_content_height_for_test(ctx),
        )
    }

    /// Whether the active composer for `path` is pinned at the 200px max-height cap.
    pub fn composer_at_max_height_for_test(&self, path: &str, ctx: &AppContext) -> Option<bool> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_at_max_height_for_test(ctx),
        )
    }

    /// Whether the flag-OFF floating composer overlay actually painted for `path` on the prior
    /// frame.
    pub fn floating_overlay_present_for_test(&self, path: &str, ctx: &AppContext) -> Option<bool> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .floating_overlay_present_for_test(ctx),
        )
    }

    /// The viewport-space Y offset at which the flag-OFF floating composer overlay is anchored for
    /// `path`, or `None` when no composer is open.
    pub fn floating_overlay_offset_for_test(&self, path: &str, ctx: &AppContext) -> Option<f32> {
        self.code_editor_for_test(path, ctx)?
            .as_ref(ctx)
            .floating_overlay_offset_for_test(ctx)
    }

    /// The set of comment ids rendered inline (as saved cards) in `path`'s editor.
    pub fn inline_comment_ids_for_test(&self, path: &str, ctx: &AppContext) -> Vec<CommentId> {
        self.code_editor_for_test(path, ctx)
            .map(|editor| editor.as_ref(ctx).inline_comment_ids_for_test())
            .unwrap_or_default()
    }

    /// Whether the active composer for `path` is editing a comment imported from GitHub.
    pub fn composer_is_imported_for_test(&self, path: &str, ctx: &AppContext) -> Option<bool> {
        Some(
            self.code_editor_for_test(path, ctx)?
                .as_ref(ctx)
                .composer_is_imported_for_test(ctx),
        )
    }

    /// The bottom panel's total comment count (all targets), via its debug state.
    pub fn panel_total_comments_for_test(&self, ctx: &AppContext) -> usize {
        self.comment_list_view
            .as_ref(ctx)
            .debug_state(ctx)
            .total_comments
    }

    /// The ids of non-outdated line-targeted comments currently in the batch (the set that should
    /// render inline). Parity check: this must equal `inline_comment_ids_for_test`.
    pub fn batch_line_comment_ids_for_test(&self, ctx: &AppContext) -> Vec<CommentId> {
        self.active_comment_model
            .as_ref()
            .map(|model| {
                model.read(ctx, |batch, _| {
                    batch
                        .comments
                        .iter()
                        .filter(|comment| {
                            !comment.outdated
                                && matches!(
                                    comment.target,
                                    AttachedReviewCommentTarget::Line { .. }
                                )
                        })
                        .map(|comment| comment.id)
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// Seed a saved comment directly into the active batch (simulating an external/import upsert,
    /// no composer interaction). When `imported` is true the comment carries a GitHub origin so the
    /// reopened editor shows the imported-from-GitHub indicator. Returns the new comment id.
    pub fn upsert_line_comment_for_test(
        &mut self,
        path: &str,
        line_number: usize,
        content: &str,
        imported: bool,
        ctx: &mut ViewContext<Self>,
    ) -> Option<CommentId> {
        use crate::code_review::comments::{
            AttachedReviewComment, CommentOrigin, ImportedCommentDetails, LineDiffContent,
        };

        let repo_path = self.repo_path()?.clone();
        let absolute_file_path = repo_path.join(path);
        let line = EditorLineLocation::Current {
            line_number: LineCount::from(line_number),
            line_range: LineCount::from(line_number)..LineCount::from(line_number + 1),
        };
        let origin = if imported {
            CommentOrigin::ImportedFromGitHub(Box::new(ImportedCommentDetails {
                author: "octocat".to_string(),
                github_comment_id: "1".to_string(),
                github_parent_id: None,
                html_url: None,
            }))
        } else {
            CommentOrigin::Native
        };
        let comment = AttachedReviewComment {
            id: CommentId::new(),
            content: content.to_string(),
            target: AttachedReviewCommentTarget::Line {
                absolute_file_path,
                line,
                content: LineDiffContent::from_content(&format!("+{content}\n")),
            },
            last_update_time: chrono::Local::now(),
            base: None,
            head: None,
            outdated: false,
            origin,
        };
        let id = comment.id;
        let model = self.active_comment_model.as_ref()?.clone();
        model.update(ctx, |batch, ctx| batch.upsert_comment(comment, ctx));
        Some(id)
    }

    /// Seed a General (review-level, non-line) comment into the batch. Used to verify File/General
    /// comments stay panel-only and never render inline.
    pub fn upsert_general_comment_for_test(
        &mut self,
        content: &str,
        ctx: &mut ViewContext<Self>,
    ) -> Option<CommentId> {
        use crate::code_review::comments::AttachedReviewComment;

        let comment = AttachedReviewComment {
            id: CommentId::new(),
            content: content.to_string(),
            target: AttachedReviewCommentTarget::General,
            last_update_time: chrono::Local::now(),
            base: None,
            head: None,
            outdated: false,
            origin: Default::default(),
        };
        let id = comment.id;
        let model = self.active_comment_model.as_ref()?.clone();
        model.update(ctx, |batch, ctx| batch.upsert_comment(comment, ctx));
        Some(id)
    }
}

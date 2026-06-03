use std::sync::Arc;

use warp_core::ui::appearance::Appearance;
use warp_editor::content::buffer::InitialBufferState;
use warp_editor::model::CoreEditorModel;
use warp_editor::render::element::VerticalExpansionBehavior;
use warp_editor::render::model::{LineCount, RenderLineLocation};
use warp_util::user_input::UserInput;
use warpui::elements::new_scrollable::ScrollableAppearance;
use warpui::elements::ScrollbarWidth;
use warpui::platform::WindowStyle;
use warpui::{App, TypedActionView, ViewHandle, WindowId};

use super::{CodeEditorRenderOptions, CodeEditorView, CodeEditorViewAction};
use crate::cloud_object::model::persistence::CloudModel;
use crate::code::editor::line::EditorLineLocation;
use crate::code::editor::EditorReviewComment;
use crate::code_review::comments::{CommentId, LineDiffContent};
use crate::editor::InteractionState;
use crate::features::FeatureFlag;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::server::server_api::team::MockTeamClient;
use crate::server::server_api::workspace::MockWorkspaceClient;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::vim_registers::VimRegisters;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspace::ActiveSession;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::AuthStateProvider;

fn initialize_editor(app: &mut App) -> (WindowId, ViewHandle<CodeEditorView>) {
    initialize_settings_for_tests(app);

    // Add all required singleton models for EditorView dependencies
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| SyncedInputState::mock());
    app.add_singleton_model(|_| VimRegisters::new());
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());

    // Add mocks required by rich text editor (used in CommentEditor)
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(NotebookKeybindings::new);

    // Add UserWorkspaces mock (required by EditorView)
    let team_client_mock = Arc::new(MockTeamClient::new());
    let workspace_client_mock = Arc::new(MockWorkspaceClient::new());
    app.add_singleton_model(|ctx| {
        UserWorkspaces::mock(
            team_client_mock.clone(),
            workspace_client_mock.clone(),
            vec![],
            ctx,
        )
    });

    let (window, editor_view) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
        CodeEditorView::new(
            None,
            None,
            CodeEditorRenderOptions::new(VerticalExpansionBehavior::GrowToMaxHeight),
            ctx,
        )
        .with_horizontal_scrollbar_appearance(ScrollableAppearance::new(ScrollbarWidth::Auto, true))
    });

    (window, editor_view)
}

const MULTILINE_CONTENT: &str = "line one\nline two\nline three\nline four\nline five\nline six\n";

fn current_line(line_number: usize) -> EditorLineLocation {
    EditorLineLocation::Current {
        line_number: LineCount::from(line_number),
        line_range: LineCount::from(line_number)..LineCount::from(line_number + 1),
    }
}

fn editor_comment(id: CommentId, line_number: usize, body: &str) -> EditorReviewComment {
    EditorReviewComment::new_with_id(
        id,
        current_line(line_number),
        LineDiffContent::default(),
        body.to_string(),
    )
}

/// VAL-SAVED-010: the editor's per-comment inline `ViewHandle` map reconciles create/update/drop
/// exactly like `CommentListView::set_comments_internal`: a reused id keeps its handle's entity id
/// stable (and refreshes its rendered body in place), a new id creates a handle, and a removed id
/// drops its handle.
#[test]
fn test_inline_comment_views_reconcile_create_update_drop() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });

        let id_a = CommentId::new();
        let id_b = CommentId::new();

        // (1) Feed [A]: one handle created for A.
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![editor_comment(id_a, 1, "alpha")].into_iter(), ctx);
        });
        let a_entity_first = app.read(|ctx| {
            let view = editor.as_ref(ctx);
            assert_eq!(view.inline_comments.len(), 1);
            view.inline_comments.get(&id_a).map(|handle| handle.id())
        });
        assert!(a_entity_first.is_some(), "A should have a handle");

        // (1 cont.) Feed [A, B]: A's handle entity id is stable, B is added.
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![
                    editor_comment(id_a, 1, "alpha"),
                    editor_comment(id_b, 2, "beta"),
                ]
                .into_iter(),
                ctx,
            );
        });
        let a_entity_second = app.read(|ctx| {
            let view = editor.as_ref(ctx);
            assert_eq!(view.inline_comments.len(), 2);
            assert!(
                view.inline_comments.contains_key(&id_b),
                "B should be added"
            );
            view.inline_comments.get(&id_a).map(|handle| handle.id())
        });
        assert_eq!(
            a_entity_first, a_entity_second,
            "A's handle must be reused (entity id stable) when fed again"
        );

        // (2) Feed [A] with a changed body: A's handle stays stable while its rendered body updates.
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![editor_comment(id_a, 1, "alpha edited")].into_iter(),
                ctx,
            );
        });
        let (a_entity_third, a_body) = app.read(|ctx| {
            let view = editor.as_ref(ctx);
            let handle = view.inline_comments.get(&id_a).expect("A still present");
            (handle.id(), handle.as_ref(ctx).rendered_body(ctx))
        });
        assert_eq!(
            a_entity_first,
            Some(a_entity_third),
            "A's handle must NOT be recreated on update"
        );
        assert_eq!(
            a_body.trim(),
            "alpha edited",
            "A's rendered body must reflect the updated content"
        );

        // (3) B is dropped now that it is no longer in the fed set.
        app.read(|ctx| {
            let view = editor.as_ref(ctx);
            assert_eq!(view.inline_comments.len(), 1);
            assert!(
                !view.inline_comments.contains_key(&id_b),
                "B's handle must be dropped when removed from the set"
            );
        });
    });
}

/// VAL-ISOLATION-004 (saved-comment half): with the flag OFF, pushing saved comments via
/// `set_comment_locations` creates NO inline comment views (only the gutter markers persist).
#[test]
fn test_inline_comment_views_not_created_when_flag_off() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(false);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![editor_comment(id_a, 1, "alpha")].into_iter(), ctx);
        });

        app.read(|ctx| {
            let view = editor.as_ref(ctx);
            assert_eq!(
                view.comment_locations.len(),
                1,
                "gutter markers should still be set while the flag is off"
            );
            assert!(
                view.inline_comments.is_empty(),
                "no inline comment views should be created while the flag is off"
            );
        });
    });
}

/// `clear_comment_locations` tears down both the gutter markers and the inline views.
#[test]
fn test_clear_comment_locations_drops_inline_views() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![editor_comment(id_a, 1, "alpha")].into_iter(), ctx);
        });
        app.read(|ctx| assert_eq!(editor.as_ref(ctx).inline_comments.len(), 1));

        editor.update(&mut app, |view, ctx| view.clear_comment_locations(ctx));
        app.read(|ctx| {
            let view = editor.as_ref(ctx);
            assert!(view.comment_locations.is_empty());
            assert!(view.inline_comments.is_empty());
        });
    });
}

/// Pump the executor until both the inner composer editor and the outer code editor have finished
/// laying out, so the inline comment block has converged on its measured height.
async fn settle_layout(app: &mut App, editor: &ViewHandle<CodeEditorView>) {
    for _ in 0..6 {
        let (inner_rs, outer_rs) = app.read(|ctx| {
            let view = editor.as_ref(ctx);
            (
                view.active_comment_editor
                    .as_ref(ctx)
                    .inner_render_state(ctx),
                view.model.as_ref(ctx).render_state().clone(),
            )
        });
        app.read(|ctx| inner_rs.as_ref(ctx).layout_complete()).await;
        app.read(|ctx| outer_rs.as_ref(ctx).layout_complete()).await;
    }
}

async fn await_outer_layout(app: &mut App, editor: &ViewHandle<CodeEditorView>) {
    let outer_rs = app.read(|ctx| editor.as_ref(ctx).model.as_ref(ctx).render_state().clone());
    app.read(|ctx| outer_rs.as_ref(ctx).layout_complete()).await;
}

/// Return the composer to a quiescent state at the end of a test: close it (tearing down the inline
/// comment block and stopping the layout-observe that re-measures it) and await the final layout so
/// no background relayout is still in flight when the test future returns.
async fn teardown_composer(app: &mut App, editor: &ViewHandle<CodeEditorView>) {
    editor.update(app, |view, ctx| {
        view.active_comment_editor.update(ctx, |composer, ctx| {
            use crate::code::editor::comment_editor::CommentEditorAction;
            composer.handle_action(&CommentEditorAction::CloseEditor, ctx);
        });
    });
    settle_layout(app, editor).await;
}

fn line_offset(app: &App, editor: &ViewHandle<CodeEditorView>, line: usize) -> f32 {
    app.read(|ctx| {
        editor
            .as_ref(ctx)
            .model
            .as_ref(ctx)
            .render_state()
            .as_ref(ctx)
            .vertical_offset_at_render_location(RenderLineLocation::Current(LineCount::from(line)))
            .map(|p| p.as_f32())
            .unwrap_or_default()
    })
}

fn comment_block_height(
    app: &App,
    editor: &ViewHandle<CodeEditorView>,
    line: usize,
) -> Option<f32> {
    app.read(|ctx| {
        editor
            .as_ref(ctx)
            .model
            .as_ref(ctx)
            .render_state()
            .as_ref(ctx)
            .comment_block_position(RenderLineLocation::Current(LineCount::from(line)))
            .map(|position| position.content_height.as_f32())
    })
}

/// VAL-COMPOSER-001/002: with the flag ON, opening the composer inline reserves real vertical
/// space at the clicked line and pushes the line below it down by the composer's height.
#[test]
fn test_inline_composer_pushes_lines_down_when_flag_on() {
    App::test((), |mut app| async move {
        let _inline = FeatureFlag::InlineCodeReview.override_enabled(true);
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        let baseline_line_3 = line_offset(&app, &editor, 3);
        assert!(
            comment_block_height(&app, &editor, 2).is_none(),
            "no inline composer block should exist before opening"
        );

        editor.update(&mut app, |view, ctx| {
            view.handle_action(
                &CodeEditorViewAction::NewCommentOnLine {
                    line: current_line(2),
                },
                ctx,
            );
        });
        settle_layout(&mut app, &editor).await;

        let block_height = comment_block_height(&app, &editor, 2)
            .expect("an inline composer block should exist at the opened line");
        assert!(
            block_height > 0.0,
            "the inline composer must reserve positive height, got {block_height}"
        );

        let shifted_line_3 = line_offset(&app, &editor, 3);
        let delta = shifted_line_3 - baseline_line_3;
        assert!(
            (delta - block_height).abs() < 1.0,
            "line below should shift down by the composer height: delta={delta}, block_height={block_height}"
        );

        teardown_composer(&mut app, &editor).await;
    });
}

/// VAL-COMPOSER-011 / VAL-ISOLATION-004 (composer half): with the flag OFF, opening the composer
/// must NOT create an inline comment block, and lines below must not shift (the floating overlay is
/// used instead).
#[test]
fn test_inline_composer_not_inline_when_flag_off() {
    App::test((), |mut app| async move {
        let _inline = FeatureFlag::InlineCodeReview.override_enabled(true);
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(false);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        let baseline_line_3 = line_offset(&app, &editor, 3);

        editor.update(&mut app, |view, ctx| {
            view.handle_action(
                &CodeEditorViewAction::NewCommentOnLine {
                    line: current_line(2),
                },
                ctx,
            );
        });
        settle_layout(&mut app, &editor).await;

        assert!(
            comment_block_height(&app, &editor, 2).is_none(),
            "no inline comment block must exist while the flag is off"
        );
        let line_3 = line_offset(&app, &editor, 3);
        assert!(
            (line_3 - baseline_line_3).abs() < 1.0,
            "line below must not shift while the flag is off: baseline={baseline_line_3}, after={line_3}"
        );

        teardown_composer(&mut app, &editor).await;
    });
}

/// VAL-COMPOSER-006: cancelling the composer removes the inline block and restores layout.
#[test]
fn test_inline_composer_cancel_restores_layout() {
    App::test((), |mut app| async move {
        let _inline = FeatureFlag::InlineCodeReview.override_enabled(true);
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;
        let baseline_line_3 = line_offset(&app, &editor, 3);

        editor.update(&mut app, |view, ctx| {
            view.handle_action(
                &CodeEditorViewAction::NewCommentOnLine {
                    line: current_line(2),
                },
                ctx,
            );
        });
        settle_layout(&mut app, &editor).await;
        assert!(comment_block_height(&app, &editor, 2).is_some());

        // Cancel via the comment editor's close action.
        editor.update(&mut app, |view, ctx| {
            view.active_comment_editor.update(ctx, |composer, ctx| {
                use crate::code::editor::comment_editor::CommentEditorAction;
                composer.handle_action(&CommentEditorAction::CloseEditor, ctx);
            });
        });
        settle_layout(&mut app, &editor).await;

        assert!(
            comment_block_height(&app, &editor, 2).is_none(),
            "cancelling should remove the inline composer block"
        );
        let line_3 = line_offset(&app, &editor, 3);
        assert!(
            (line_3 - baseline_line_3).abs() < 1.0,
            "layout should be restored after cancel: baseline={baseline_line_3}, after={line_3}"
        );

        teardown_composer(&mut app, &editor).await;
    });
}

#[test]
fn test_interaction_state_prevents_editing() {
    App::test((), |mut app| async move {
        let (_window, editor_view) = initialize_editor(&mut app);

        let text = editor_view.update(&mut app, |view, ctx| {
            view.handle_action(&CodeEditorViewAction::UserTyped(UserInput::new("abc")), ctx);
            view.text(ctx)
        });

        assert_eq!(text.as_str(), "abc");

        // Set to be only selectable
        editor_view.update(&mut app, |view, ctx| {
            view.set_interaction_state(InteractionState::Selectable, ctx);
        });

        let text = editor_view.update(&mut app, |view, ctx| {
            view.handle_action(&CodeEditorViewAction::UserTyped(UserInput::new("def")), ctx);
            view.text(ctx)
        });

        assert_eq!(text.as_str(), "abc");
    });
}

// --- Saved comments rendered inline (VAL-SAVED-*) ---------------------------------------------

/// Pump the executor until the outer editor and every saved inline card's body editor have finished
/// laying out, so each inline comment block has converged on its measured height.
async fn settle_saved_layout(app: &mut App, editor: &ViewHandle<CodeEditorView>) {
    for _ in 0..8 {
        let states = app.read(|ctx| {
            let view = editor.as_ref(ctx);
            let mut states = vec![view.model.as_ref(ctx).render_state().clone()];
            for inline in view.inline_comments.values() {
                states.push(inline.as_ref(ctx).inner_render_state(ctx));
            }
            states
        });
        for state in states {
            app.read(|ctx| state.as_ref(ctx).layout_complete()).await;
        }
    }
}

fn comment_block_count(app: &App, editor: &ViewHandle<CodeEditorView>) -> usize {
    app.read(|ctx| {
        editor
            .as_ref(ctx)
            .model
            .as_ref(ctx)
            .render_state()
            .as_ref(ctx)
            .comment_block_count()
    })
}

/// VAL-SAVED-001/002: with the flag ON, a saved line comment renders as an inline block that
/// reserves real vertical space at its line and pushes the line below it down by the block height.
#[test]
fn test_saved_comment_renders_inline_and_pushes_lines_down() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;
        let baseline_line_3 = line_offset(&app, &editor, 3);
        assert!(comment_block_height(&app, &editor, 2).is_none());

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![editor_comment(id_a, 2, "INLINE BODY ALPHA")].into_iter(),
                ctx,
            );
        });
        settle_saved_layout(&mut app, &editor).await;

        let block_height = comment_block_height(&app, &editor, 2)
            .expect("a saved inline block should exist at the comment line");
        assert!(
            block_height > 0.0,
            "the saved card must reserve positive height, got {block_height}"
        );
        assert_eq!(comment_block_count(&app, &editor), 1);

        let shifted_line_3 = line_offset(&app, &editor, 3);
        let delta = shifted_line_3 - baseline_line_3;
        assert!(
            (delta - block_height).abs() < 1.0,
            "line below should shift down by the card height: delta={delta}, block_height={block_height}"
        );
    });
}

/// VAL-SAVED-003: three saved comments on distinct lines each render as their own inline block.
#[test]
fn test_multiple_saved_comments_render_each_at_own_line() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        let (id_a, id_b, id_c) = (CommentId::new(), CommentId::new(), CommentId::new());
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![
                    editor_comment(id_a, 1, "alpha"),
                    editor_comment(id_b, 3, "beta"),
                    editor_comment(id_c, 5, "gamma"),
                ]
                .into_iter(),
                ctx,
            );
        });
        settle_saved_layout(&mut app, &editor).await;

        assert_eq!(comment_block_count(&app, &editor), 3);
        for line in [1usize, 3, 5] {
            assert!(
                comment_block_height(&app, &editor, line).is_some(),
                "expected an inline block at line {line}"
            );
        }
    });
}

/// VAL-SAVED-004: editing a saved comment updates the existing inline block in place (no duplicate).
#[test]
fn test_editing_saved_comment_updates_block_in_place() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![editor_comment(id_a, 2, "before")].into_iter(), ctx);
        });
        settle_saved_layout(&mut app, &editor).await;
        assert_eq!(comment_block_count(&app, &editor), 1);

        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![editor_comment(id_a, 2, "after edit")].into_iter(),
                ctx,
            );
        });
        settle_saved_layout(&mut app, &editor).await;

        assert_eq!(
            comment_block_count(&app, &editor),
            1,
            "editing must not create a second inline block"
        );
        let body = app.read(|ctx| {
            editor
                .as_ref(ctx)
                .inline_comments
                .get(&id_a)
                .expect("comment still present")
                .as_ref(ctx)
                .rendered_body(ctx)
        });
        assert_eq!(body.trim(), "after edit");
    });
}

/// VAL-SAVED-005: deleting a saved comment removes its inline block and restores the line layout.
#[test]
fn test_deleting_saved_comment_removes_block_and_restores_layout() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;
        let baseline_line_3 = line_offset(&app, &editor, 3);

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![editor_comment(id_a, 2, "to delete")].into_iter(), ctx);
        });
        settle_saved_layout(&mut app, &editor).await;
        assert_eq!(comment_block_count(&app, &editor), 1);
        assert!(line_offset(&app, &editor, 3) > baseline_line_3);

        // Remove the comment from the fed set (mirrors a batch delete).
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(Vec::new().into_iter(), ctx);
        });
        settle_saved_layout(&mut app, &editor).await;

        assert_eq!(comment_block_count(&app, &editor), 0);
        let restored_line_3 = line_offset(&app, &editor, 3);
        assert!(
            (restored_line_3 - baseline_line_3).abs() < 1.0,
            "layout should be restored after delete: baseline={baseline_line_3}, after={restored_line_3}"
        );
    });
}

/// VAL-SAVED-012 / VAL-ISOLATION-004 (saved half): with the flag OFF, a saved comment produces NO
/// inline block.
#[test]
fn test_saved_comment_no_inline_block_when_flag_off() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(false);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;
        let baseline_line_3 = line_offset(&app, &editor, 3);

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![editor_comment(id_a, 2, "alpha")].into_iter(), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        assert_eq!(comment_block_count(&app, &editor), 0);
        let line_3 = line_offset(&app, &editor, 3);
        assert!(
            (line_3 - baseline_line_3).abs() < 1.0,
            "no line should shift while the flag is off"
        );
    });
}

/// VAL-SAVED-015: an imported-from-GitHub saved comment renders inline as a saved card (with its
/// GitHub affordance) without panicking or thrashing layout.
#[test]
fn test_imported_saved_comment_renders_inline() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        let id_a = CommentId::new();
        let mut comment = editor_comment(id_a, 2, "imported body");
        comment.origin = crate::code_review::comments::CommentOrigin::ImportedFromGitHub(Box::new(
            crate::code_review::comments::ImportedCommentDetails {
                author: "octocat".to_string(),
                github_comment_id: "1".to_string(),
                github_parent_id: None,
                html_url: None,
            },
        ));
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(vec![comment].into_iter(), ctx);
        });
        settle_saved_layout(&mut app, &editor).await;

        assert_eq!(comment_block_count(&app, &editor), 1);
    });
}

/// VAL-EDGE-001: two distinct saved comments anchored to the SAME current line both render inline,
/// stacked as two distinct blocks; the code line below the anchor is pushed down by the SUM of both
/// blocks' heights (they do not overlap each other or the code).
#[test]
fn test_two_saved_comments_stack_on_same_line() {
    App::test((), |mut app| async move {
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;
        let baseline_line_3 = line_offset(&app, &editor, 3);

        let (id_a, id_b) = (CommentId::new(), CommentId::new());
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![
                    editor_comment(id_a, 2, "first comment"),
                    editor_comment(id_b, 2, "second comment"),
                ]
                .into_iter(),
                ctx,
            );
        });
        settle_saved_layout(&mut app, &editor).await;

        // Both same-line comments render as their own inline block.
        assert_eq!(
            comment_block_count(&app, &editor),
            2,
            "two same-line comments must render as two distinct blocks"
        );

        // The line below the anchor is pushed down by the SUM of both cards' heights.
        let summed_height = app.read(|ctx| {
            let view = editor.as_ref(ctx);
            view.inline_comments
                .values()
                .map(|inline| inline.as_ref(ctx).inline_height(ctx).as_f32())
                .sum::<f32>()
        });
        let delta = line_offset(&app, &editor, 3) - baseline_line_3;
        assert!(
            (delta - summed_height).abs() < 1.0,
            "line below should shift by the summed card heights: delta={delta}, summed={summed_height}"
        );
    });
}

/// VAL-COMPOSER-015: opening the composer on a line that already has a saved card REPLACES the card
/// (exactly one inline block — the composer), and cancelling restores the single saved card.
#[test]
fn test_composer_replaces_saved_card_on_same_line() {
    App::test((), |mut app| async move {
        let _inline = FeatureFlag::InlineCodeReview.override_enabled(true);
        let _embedded = FeatureFlag::EmbeddedCodeReviewComments.override_enabled(true);

        let (_window, editor) = initialize_editor(&mut app);
        editor.update(&mut app, |view, ctx| {
            view.reset(InitialBufferState::plain_text(MULTILINE_CONTENT), ctx);
        });
        await_outer_layout(&mut app, &editor).await;

        let id_a = CommentId::new();
        editor.update(&mut app, |view, ctx| {
            view.set_comment_locations(
                vec![editor_comment(id_a, 2, "saved body")].into_iter(),
                ctx,
            );
        });
        settle_saved_layout(&mut app, &editor).await;
        assert_eq!(comment_block_count(&app, &editor), 1);

        // Open the composer on the same line: the card is replaced, not stacked.
        editor.update(&mut app, |view, ctx| {
            view.handle_action(
                &CodeEditorViewAction::NewCommentOnLine {
                    line: current_line(2),
                },
                ctx,
            );
        });
        settle_layout(&mut app, &editor).await;
        assert_eq!(
            comment_block_count(&app, &editor),
            1,
            "composer must replace the saved card, not stack with it"
        );

        // Cancel: the single saved card returns.
        editor.update(&mut app, |view, ctx| {
            view.active_comment_editor.update(ctx, |composer, ctx| {
                use crate::code::editor::comment_editor::CommentEditorAction;
                composer.handle_action(&CommentEditorAction::CloseEditor, ctx);
            });
        });
        settle_saved_layout(&mut app, &editor).await;
        assert_eq!(
            comment_block_count(&app, &editor),
            1,
            "the saved card returns after the composer is cancelled"
        );
    });
}

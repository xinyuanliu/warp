use std::sync::Arc;

use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::watcher::DirectoryWatcher;
#[cfg(feature = "local_fs")]
use repo_metadata::RepoMetadataModel;
use serde_json::Value;
use warp_core::ui::appearance::Appearance;
use warp_editor::model::CoreEditorModel;
#[cfg(feature = "local_fs")]
use warp_files::FileModel;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::platform::WindowStyle;
use warpui::{App, TypedActionView, View};

use super::super::ipynb_model::CellKind;
use super::{InsertPosition, JupyterNotebookAction, JupyterNotebookView};
use crate::auth::auth_manager::AuthManager;
use crate::auth::AuthStateProvider;
use crate::cloud_object::model::persistence::CloudModel;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::search::files::model::FileSearchModel;
use crate::server::server_api::team::MockTeamClient;
use crate::server::server_api::workspace::MockWorkspaceClient;
use crate::server::server_api::ServerApiProvider;
use crate::server::telemetry::context_provider::AppTelemetryContextProvider;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::terminal::keys::TerminalKeybindings;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspace::ActiveSession;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::{GlobalResourceHandles, GlobalResourceHandlesProvider};

/// A small but representative nbformat v4 notebook: a markdown cell, followed by
/// a code cell with a saved stream output.
const SIMPLE_NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "markdown",
   "metadata": {},
   "source": ["# Title\n", "\n", "Some *text*."]
  },
  {
   "cell_type": "code",
   "execution_count": 3,
   "metadata": {},
   "outputs": [
    {"name": "stdout", "output_type": "stream", "text": ["hello\n"]}
   ],
   "source": ["print('hello')"]
  }
 ],
 "metadata": {
  "language_info": {"name": "python", "version": "3.11.0"}
 },
 "nbformat": 4,
 "nbformat_minor": 5
}"##;

fn init_app(app: &mut App) {
    initialize_settings_for_tests(app);

    let global_resource_handles = GlobalResourceHandles::mock(app);
    app.add_singleton_model(|_| GlobalResourceHandlesProvider::new(global_resource_handles));
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    app.add_singleton_model(DirectoryWatcher::new);
    app.add_singleton_model(|_| DetectedRepositories::default());
    #[cfg(feature = "local_fs")]
    app.add_singleton_model(RepoMetadataModel::new);
    app.add_singleton_model(FileSearchModel::new);
    #[cfg(feature = "local_fs")]
    app.add_singleton_model(FileModel::new);
    app.add_singleton_model(NotebookKeybindings::new);
    app.add_singleton_model(TerminalKeybindings::new);
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
    app.add_singleton_model(AuthManager::new_for_test);
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
    #[cfg(feature = "voice_input")]
    app.add_singleton_model(voice_input::VoiceInput::new);
}

/// Build a view over the given notebook content in a fresh test window.
fn add_view(app: &mut App, content: &'static str) -> warpui::ViewHandle<JupyterNotebookView> {
    let (_, handle) = app.add_window(WindowStyle::NotStealFocus, move |ctx| {
        JupyterNotebookView::new(content, None, ctx)
    });
    handle
}

#[test]
fn unparseable_file_falls_back_to_raw_text() {
    // Invariant 16: a malformed notebook never shows a blank view or panics; it
    // falls back to the raw text, which `to_json` returns verbatim.
    App::test((), |mut app| async move {
        init_app(&mut app);
        let bad = "{ this is not valid notebook json";
        let handle = add_view(&mut app, bad);

        handle.read(&app, |view, ctx| {
            assert!(view.is_fallback());
            assert_eq!(view.cell_count(), 0);
            assert_eq!(view.to_json(), bad);
            // Rendering the fallback must not panic.
            view.render(ctx);
        });
    });
}

#[test]
fn parsed_notebook_round_trips_unedited() {
    // Invariants 11/18: an unedited notebook serializes back without changing
    // semantic content.
    App::test((), |mut app| async move {
        init_app(&mut app);
        let handle = add_view(&mut app, SIMPLE_NOTEBOOK);

        handle.read(&app, |view, ctx| {
            assert!(!view.is_fallback());
            assert_eq!(view.cell_count(), 2);
            assert_eq!(view.cell_kind(0), Some(CellKind::Markdown));
            assert_eq!(view.cell_kind(1), Some(CellKind::Code));
            assert!(!view.is_dirty());

            let reserialized: Value =
                serde_json::from_str(&view.to_json()).expect("serialized notebook is valid JSON");
            let original: Value =
                serde_json::from_str(SIMPLE_NOTEBOOK).expect("fixture is valid JSON");
            assert_eq!(reserialized, original);

            // Rendering must not panic.
            view.render(ctx);
        });
    });
}

#[test]
fn code_cell_outputs_render_read_only() {
    // Invariant 5: a code cell's saved outputs render beneath it as read-only
    // elements (not editors).
    App::test((), |mut app| async move {
        init_app(&mut app);
        let handle = add_view(&mut app, SIMPLE_NOTEBOOK);

        handle.read(&app, |view, _| {
            // The markdown cell has no outputs; the code cell has its one stream output.
            assert_eq!(view.output_count(0), 0);
            assert_eq!(view.output_count(1), 1);
        });
    });
}

#[test]
fn editing_code_cell_updates_only_that_source_and_preserves_outputs() {
    // Invariants 7/9: editing a code cell updates only its source and leaves its
    // saved outputs untouched.
    App::test((), |mut app| async move {
        init_app(&mut app);
        let handle = add_view(&mut app, SIMPLE_NOTEBOOK);

        let code_editor = handle.read(&app, |view, _| {
            view.code_editor_at(1).expect("cell 1 is a code cell")
        });

        // Simulate a user edit (user-origin insert), which fires a
        // content-changed event that the notebook view syncs back into the model.
        code_editor.update(&mut app, |editor, ctx| {
            editor.model.update(ctx, |model, ctx| {
                model.user_insert("print('world')\n", ctx);
            });
        });

        handle.read(&app, |view, _| {
            assert!(view.is_dirty());
            let source = view.cell_source(1).expect("code cell source");
            assert!(
                source.contains("print('world')"),
                "edited source should be reflected, got: {source:?}"
            );
            // The markdown cell was not touched.
            assert_eq!(
                view.cell_source(0).as_deref(),
                Some("# Title\n\nSome *text*.")
            );

            // Saved outputs are preserved byte-for-byte on save (invariant 9).
            let json: Value = serde_json::from_str(&view.to_json()).unwrap();
            let outputs = &json["cells"][1]["outputs"];
            assert_eq!(outputs[0]["output_type"], "stream");
            assert_eq!(outputs[0]["text"][0], "hello\n");
        });
    });
}

#[test]
fn editing_markdown_cell_updates_only_that_source() {
    // Invariant 6: editing a markdown cell updates only that cell.
    App::test((), |mut app| async move {
        init_app(&mut app);
        let handle = add_view(&mut app, SIMPLE_NOTEBOOK);

        let md_editor = handle.read(&app, |view, _| {
            view.markdown_editor_at(0)
                .expect("cell 0 is a markdown cell")
        });

        // Type into the markdown editor (editable without focus in tests).
        md_editor.update(&mut app, |editor, ctx| {
            editor.user_typed("Z", ctx);
        });

        handle.read(&app, |view, _| {
            assert!(view.is_dirty());
            let source = view.cell_source(0).expect("markdown cell source");
            assert!(
                source.contains('Z'),
                "edited markdown should be reflected, got: {source:?}"
            );
            // The code cell was not touched.
            assert_eq!(view.cell_source(1).as_deref(), Some("print('hello')"));
        });
    });
}

#[test]
fn reload_replaces_content_and_clears_dirty() {
    // set_content reloads fresh content and resets dirty state (invariant 13 reload path).
    App::test((), |mut app| async move {
        init_app(&mut app);
        let handle = add_view(&mut app, SIMPLE_NOTEBOOK);

        let other = r##"{
 "cells": [{"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["x = 1"]}],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}"##;

        handle.update(&mut app, |view, ctx| {
            // Dirty the view, then reload.
            view.handle_action(
                &JupyterNotebookAction::InsertCell {
                    anchor: None,
                    position: InsertPosition::Below,
                    kind: CellKind::Markdown,
                },
                ctx,
            );
            assert!(view.is_dirty());

            view.set_content(other, ctx);
            assert!(!view.is_dirty());
            assert_eq!(view.cell_count(), 1);
            assert_eq!(view.cell_kind(0), Some(CellKind::Code));
        });
    });
}

#[test]
fn structural_operations_insert_delete_move_convert() {
    // Invariant 8: insert above/below, delete, move, and convert cells.
    App::test((), |mut app| async move {
        init_app(&mut app);
        let handle = add_view(&mut app, SIMPLE_NOTEBOOK);

        handle.update(&mut app, |view, ctx| {
            // Insert a code cell ABOVE the first cell.
            view.handle_action(
                &JupyterNotebookAction::InsertCell {
                    anchor: view.cell_id_at(0),
                    position: InsertPosition::Above,
                    kind: CellKind::Code,
                },
                ctx,
            );
            assert_eq!(view.cell_count(), 3);
            assert_eq!(view.cell_kind(0), Some(CellKind::Code));
            assert!(view.is_dirty());

            view.mark_saved(ctx);
            assert!(!view.is_dirty());

            // Convert the inserted code cell to markdown.
            let first = view.cell_id_at(0).expect("first cell id");
            view.handle_action(
                &JupyterNotebookAction::ConvertCell {
                    id: first,
                    kind: CellKind::Markdown,
                },
                ctx,
            );
            assert_eq!(view.cell_kind(0), Some(CellKind::Markdown));

            // Move it down, then delete it.
            let first = view.cell_id_at(0).expect("first cell id");
            view.handle_action(&JupyterNotebookAction::MoveCellDown(first), ctx);
            assert_eq!(view.cell_kind(1), Some(CellKind::Markdown));

            let id = view.cell_id_at(1).expect("cell id");
            view.handle_action(&JupyterNotebookAction::DeleteCell(id), ctx);
            assert_eq!(view.cell_count(), 2);

            // Focus/Close are valid actions and must not panic.
            view.handle_action(&JupyterNotebookAction::Focus, ctx);
            view.handle_action(&JupyterNotebookAction::Close, ctx);
        });
    });
}

#[test]
fn path_is_exposed_and_titled() {
    App::test((), |mut app| async move {
        init_app(&mut app);
        let (_, handle) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
            JupyterNotebookView::new(
                SIMPLE_NOTEBOOK,
                Some(LocalOrRemotePath::Local("/tmp/demo/analysis.ipynb".into())),
                ctx,
            )
        });
        handle.read(&app, |view, _| {
            assert!(view.path().is_some());
            assert_eq!(view.title(), "analysis.ipynb");
        });
    });
}

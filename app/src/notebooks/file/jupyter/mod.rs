//! Editable, cell-based Jupyter (`.ipynb`) notebook view.
//!
//! Given the raw contents of a `.ipynb` file, [`JupyterNotebookView`] parses it
//! into a lossless [`NotebookDoc`] and renders each cell as an editable surface:
//! markdown cells as rendered/editable markdown, code cells as editable
//! syntax-highlighted source, with each code cell's saved `outputs` shown
//! read-only beneath it (product_v1.md invariants 3-5). Editing a cell updates
//! only that cell's `source`; saved `outputs` and untouched cells are never
//! altered (invariants 6, 7, 9). If the file is not a parseable v4 notebook the
//! view falls back to showing the raw text in an editable code editor — never a
//! blank view, never a panic (invariant 16).
//!
//! All behavior here is gated by the caller behind the
//! `JupyterNotebookEditing` feature flag.

use std::path::Path;

use serde_json::Value;
use warp_editor::content::buffer::InitialBufferState;
use warp_editor::render::element::VerticalExpansionBehavior;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::accessibility::{AccessibilityContent, WarpA11yRole};
use warpui::assets::asset_cache::{AssetCache, AssetSource};
use warpui::elements::new_scrollable::{NewScrollable, SingleAxisConfig};
use warpui::elements::{
    Align, Border, CacheOption, ChildView, ClippedScrollStateHandle, Container, CornerRadius,
    CrossAxisAlignment, Expanded, Flex, Image, MainAxisSize, MouseStateHandle, ParentElement,
    Radius, Text,
};
use warpui::image_cache::ImageType;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::UiComponent;
use warpui::{
    AppContext, Element, Entity, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use super::ipynb_model::{CellDoc, CellKind, NotebookDoc};
use crate::appearance::Appearance;
use crate::editor::InteractionState;
use crate::menu::{MenuItem, MenuItemFields};
use crate::notebooks::editor::model::NotebooksEditorModel;
use crate::notebooks::editor::rich_text_styles;
use crate::notebooks::editor::view::{EditorViewEvent, RichTextEditorConfig, RichTextEditorView};
use crate::notebooks::link::{NotebookLinks, SessionSource};
use crate::pane_group::focus_state::PaneFocusHandle;
use crate::pane_group::pane::view;
use crate::pane_group::{BackingView, PaneConfiguration, PaneEvent};
use crate::settings::FontSettings;

mod output;
use output::OutputItem;

/// Font size used for read-only preformatted output text.
const OUTPUT_FONT_SIZE: f32 = 12.0;

/// A stable identifier for a cell view, independent of its position. Structural
/// operations (insert/delete/move) shift cell indices, so editor event handlers
/// and per-cell actions reference cells by this id rather than by index.
///
/// Opaque: it appears in the public [`JupyterNotebookAction`] but carries no
/// host-meaningful value (the inner counter is private).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellId(u64);

/// Where a newly inserted cell goes relative to an anchor cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPosition {
    Above,
    Below,
}

/// Actions handled by the notebook view, including cell structural operations
/// (product_v1.md invariant 8). Also used as the pane header overflow-menu
/// action type for [`BackingView`].
#[derive(Debug, Clone)]
pub enum JupyterNotebookAction {
    /// Focus the notebook view.
    Focus,
    /// Close the pane hosting this view.
    Close,
    /// Persist the current notebook (emits [`JupyterNotebookEvent::SaveRequested`]).
    Save,
    /// Toggle the pane between maximized and restored.
    ToggleMaximized,
    /// Insert a new empty cell relative to `anchor` (or at an end when `None`).
    InsertCell {
        anchor: Option<CellId>,
        position: InsertPosition,
        kind: CellKind,
    },
    /// Delete the cell with the given id.
    DeleteCell(CellId),
    /// Move the cell with the given id one position earlier.
    MoveCellUp(CellId),
    /// Move the cell with the given id one position later.
    MoveCellDown(CellId),
    /// Convert the cell with the given id to a different kind.
    ConvertCell { id: CellId, kind: CellKind },
}

/// Events emitted to the host pane.
#[derive(Debug, Clone)]
pub enum JupyterNotebookEvent {
    /// Emitted once when the notebook transitions from clean to having unsaved
    /// edits (invariant 10).
    Dirtied,
    /// The user requested a save. `json` is the full serialized `.ipynb` to
    /// persist; the host writes it and then calls [`JupyterNotebookView::mark_saved`].
    SaveRequested { json: String },
    /// The file is not a parseable v4 notebook, so the host should open it in a
    /// real code editor as plain text (invariant 16). `json` is the raw file
    /// content. Emitted only at load time, so there are no unsaved edits to lose.
    RawRequested { json: String },
    /// Focus entered the notebook view.
    Focused,
    /// A pane-level event for the host pane to act on.
    Pane(PaneEvent),
}

impl From<PaneEvent> for JupyterNotebookEvent {
    fn from(event: PaneEvent) -> Self {
        JupyterNotebookEvent::Pane(event)
    }
}

/// Mouse-state handles for a single cell's structural-operation buttons.
/// Created once when a cell view is built (never inline during render).
#[derive(Default)]
struct CellToolbarHandles {
    move_up: MouseStateHandle,
    move_down: MouseStateHandle,
    convert: MouseStateHandle,
    insert_markdown: MouseStateHandle,
    insert_code: MouseStateHandle,
    delete: MouseStateHandle,
}

/// The editable surface for a single cell.
enum CellViewKind {
    /// Markdown cell rendered as editable WYSIWYG markdown.
    Markdown(ViewHandle<RichTextEditorView>),
    /// Code cell rendered as an editable, syntax-highlighted editor.
    Code(ViewHandle<CodeEditorView>),
    /// Raw (or unknown-kind) cell rendered as editable plain text.
    Raw(ViewHandle<CodeEditorView>),
}

/// A display-only, read-only output beneath a code cell.
enum RenderableOutput {
    /// Preformatted text (stream / `text/plain` / traceback / placeholder).
    Text(String),
    /// An inline image, registered in the asset cache by source.
    Image(AssetSource),
}

/// A single cell's view state. Index-aligned with the model's `cells`.
struct CellView {
    id: CellId,
    kind: CellViewKind,
    /// Pre-rendered read-only outputs (code cells only; empty otherwise).
    outputs: Vec<RenderableOutput>,
    toolbar: CellToolbarHandles,
}

/// The content state of the view: either a parsed, editable notebook, or a raw
/// fallback editor when the file is not a parseable v4 notebook (invariant 16).
enum NotebookState {
    Notebook {
        doc: NotebookDoc,
        cells: Vec<CellView>,
    },
    Raw {
        editor: ViewHandle<CodeEditorView>,
        /// The current raw text, kept in sync with the fallback editor so
        /// [`JupyterNotebookView::to_json`] reflects edits.
        text: String,
    },
}

use crate::code::editor::view::{CodeEditorEvent, CodeEditorRenderOptions, CodeEditorView};

/// Editable cell-based view of a Jupyter `.ipynb` file.
pub struct JupyterNotebookView {
    state: NotebookState,
    path: Option<LocalOrRemotePath>,
    dirty: bool,
    /// Monotonic counter backing [`CellId`]s.
    next_cell_id: u64,
    view_position_id: String,
    links: ModelHandle<NotebookLinks>,
    pane_configuration: ModelHandle<PaneConfiguration>,
    focus_handle: Option<PaneFocusHandle>,
    vertical_scroll_state: ClippedScrollStateHandle,
    /// Mouse states for the "add first cell" affordances shown when empty.
    add_first_markdown: MouseStateHandle,
    add_first_code: MouseStateHandle,
    /// The notebook's highlight language, derived from metadata.
    language: Option<String>,
}

impl JupyterNotebookView {
    /// Construct a notebook view from the raw contents of a `.ipynb` file.
    ///
    /// Parses via [`NotebookDoc::parse`]; on parse error, falls back to showing
    /// the raw text in an editable code editor (never blank, never panics —
    /// invariant 16). `path` is used for the title/identity; it may be `None`
    /// for static or untitled content.
    pub fn new(
        content: &str,
        path: Option<LocalOrRemotePath>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let window_id = ctx.window_id();
        let links = ctx.add_model(|ctx| NotebookLinks::new(SessionSource::Active(window_id), ctx));
        let view_position_id = format!("jupyter_notebook_view_{}", ctx.view_id());
        let pane_configuration = ctx.add_model(|_| PaneConfiguration::new(""));

        let mut next_cell_id = 0;
        let (state, language) =
            Self::build_state(content, &links, &view_position_id, &mut next_cell_id, ctx);

        let mut view = Self {
            state,
            path,
            dirty: false,
            next_cell_id,
            view_position_id,
            links,
            pane_configuration,
            focus_handle: None,
            vertical_scroll_state: ClippedScrollStateHandle::default(),
            add_first_markdown: MouseStateHandle::default(),
            add_first_code: MouseStateHandle::default(),
            language,
        };
        view.update_pane_title(ctx);
        view
    }

    /// Replace the notebook contents from fresh on-disk content (external
    /// reload). Re-parses and rebuilds cell sub-views and clears the dirty flag.
    pub fn set_content(&mut self, content: &str, ctx: &mut ViewContext<Self>) {
        let links = self.links.clone();
        let position_id = self.view_position_id.clone();
        let mut next_id = self.next_cell_id;
        let (state, language) = Self::build_state(content, &links, &position_id, &mut next_id, ctx);
        self.next_cell_id = next_id;
        self.state = state;
        self.language = language;
        self.dirty = false;
        // If the file isn't a parseable notebook, the internal fallback already
        // shows the raw text (never blank/panic). Additionally ask the host to
        // open it in the real code editor so it's editable/saveable as plain
        // text (invariant 16).
        if matches!(self.state, NotebookState::Raw { .. }) {
            ctx.emit(JupyterNotebookEvent::RawRequested {
                json: content.to_string(),
            });
        }
        ctx.notify();
    }

    /// The current notebook serialized to `.ipynb` JSON. Delegates to
    /// [`NotebookDoc::to_json_pretty`] for a parsed notebook, or returns the raw
    /// text verbatim in fallback mode. Cell edits are synced into the model
    /// eagerly, so this needs no context.
    pub fn to_json(&self) -> String {
        match &self.state {
            NotebookState::Notebook { doc, .. } => doc.to_json_pretty(),
            NotebookState::Raw { text, .. } => text.clone(),
        }
    }

    /// Whether there are unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear the dirty flag after the host successfully saves.
    pub fn mark_saved(&mut self, ctx: &mut ViewContext<Self>) {
        if self.dirty {
            self.dirty = false;
            ctx.notify();
        }
    }

    /// Emit a [`JupyterNotebookEvent::SaveRequested`] carrying the current JSON.
    pub fn request_save(&mut self, ctx: &mut ViewContext<Self>) {
        let json = self.to_json();
        ctx.emit(JupyterNotebookEvent::SaveRequested { json });
    }

    /// The path to the currently-open file, if any.
    pub fn path(&self) -> Option<&LocalOrRemotePath> {
        self.path.as_ref()
    }

    /// The pane configuration model, used by the host pane to build its
    /// `PaneView` and render the header title.
    pub fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    /// The display title for this notebook (file name, or "Untitled").
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .map(|path| {
                let display = path.display_path();
                Path::new(&display)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or(display)
            })
            .unwrap_or_else(|| "Untitled".to_string())
    }

    /// Focus the notebook's first editable cell (or the raw editor).
    pub fn focus(&self, ctx: &mut ViewContext<Self>) {
        match &self.state {
            NotebookState::Notebook { cells, .. } => match cells.first() {
                Some(cell) => match &cell.kind {
                    CellViewKind::Markdown(editor) => ctx.focus(editor),
                    CellViewKind::Code(editor) | CellViewKind::Raw(editor) => ctx.focus(editor),
                },
                None => ctx.focus_self(),
            },
            NotebookState::Raw { editor, .. } => ctx.focus(editor),
        }
    }

    fn update_pane_title(&mut self, ctx: &mut ViewContext<Self>) {
        let title = self.title();
        self.pane_configuration
            .update(ctx, |config, ctx| config.set_title(title, ctx));
    }

    /// Build the content state from raw file contents.
    fn build_state(
        content: &str,
        links: &ModelHandle<NotebookLinks>,
        position_id: &str,
        next_id: &mut u64,
        ctx: &mut ViewContext<Self>,
    ) -> (NotebookState, Option<String>) {
        match NotebookDoc::parse(content) {
            Ok(doc) => {
                let language = doc.language();
                let mut cells = Vec::with_capacity(doc.cells.len());
                for cell in &doc.cells {
                    cells.push(Self::build_cell_view(
                        cell,
                        language.as_deref(),
                        links,
                        position_id,
                        next_id,
                        ctx,
                    ));
                }
                (NotebookState::Notebook { doc, cells }, language)
            }
            // Not a parseable v4 notebook: fall back to editable raw text.
            Err(_) => {
                let editor = Self::build_raw_editor(content, ctx);
                (
                    NotebookState::Raw {
                        editor,
                        text: content.to_string(),
                    },
                    None,
                )
            }
        }
    }

    /// Build the view state for a single cell, wiring up the appropriate editor
    /// and (for code cells) its read-only outputs.
    fn build_cell_view(
        cell: &CellDoc,
        language: Option<&str>,
        links: &ModelHandle<NotebookLinks>,
        position_id: &str,
        next_id: &mut u64,
        ctx: &mut ViewContext<Self>,
    ) -> CellView {
        let id = CellId(*next_id);
        *next_id += 1;
        let source = cell.source_text();
        let kind = cell.kind();

        let view_kind = match kind {
            Some(CellKind::Markdown) => CellViewKind::Markdown(Self::build_markdown_editor(
                id,
                &source,
                links,
                position_id,
                ctx,
            )),
            Some(CellKind::Code) => {
                CellViewKind::Code(Self::build_code_editor(id, &source, language, ctx))
            }
            // Raw cells and unknown cell types render as plain editable text.
            Some(CellKind::Raw) | None => {
                CellViewKind::Raw(Self::build_code_editor(id, &source, None, ctx))
            }
        };

        let outputs = match kind {
            Some(CellKind::Code) => {
                Self::build_outputs(id, cell.outputs.as_deref().unwrap_or(&[]), position_id, ctx)
            }
            Some(CellKind::Markdown) | Some(CellKind::Raw) | None => Vec::new(),
        };

        CellView {
            id,
            kind: view_kind,
            outputs,
            toolbar: CellToolbarHandles::default(),
        }
    }

    fn build_markdown_editor(
        id: CellId,
        source: &str,
        links: &ModelHandle<NotebookLinks>,
        position_id: &str,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<RichTextEditorView> {
        let window_id = ctx.window_id();
        let model = ctx.add_model(|ctx| {
            let styles = rich_text_styles(Appearance::as_ref(ctx), FontSettings::as_ref(ctx));
            NotebooksEditorModel::new(styles, window_id, ctx)
        });
        let cell_position_id = format!("{position_id}_md_{}", id.0);
        let links = links.clone();
        let editor = ctx.add_typed_action_view(move |ctx| {
            let config = RichTextEditorConfig {
                disable_scrolling: true,
                vertical_expansion_behavior: Some(VerticalExpansionBehavior::InfiniteHeight),
                ..Default::default()
            };
            let mut view = RichTextEditorView::new(cell_position_id, model, links, config, ctx);
            view.reset_with_markdown(source, ctx);
            view.set_interaction_state(InteractionState::Editable, ctx);
            view
        });
        ctx.subscribe_to_view(&editor, move |me, handle, event, ctx| {
            me.handle_markdown_cell_event(id, handle, event, ctx);
        });
        editor
    }

    fn build_code_editor(
        id: CellId,
        source: &str,
        language: Option<&str>,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<CodeEditorView> {
        let editor = ctx.add_typed_action_view(move |ctx| {
            let view = CodeEditorView::new(
                None,
                None,
                CodeEditorRenderOptions::new(VerticalExpansionBehavior::InfiniteHeight),
                ctx,
            );
            view.reset(InitialBufferState::plain_text(source), ctx);
            view.set_interaction_state(InteractionState::Editable, ctx);
            view
        });
        if let Some(language) = language {
            editor.update(ctx, |view, ctx| view.set_language_with_name(language, ctx));
        }
        ctx.subscribe_to_view(&editor, move |me, handle, event, ctx| {
            me.handle_code_cell_event(id, handle, event, ctx);
        });
        editor
    }

    fn build_raw_editor(content: &str, ctx: &mut ViewContext<Self>) -> ViewHandle<CodeEditorView> {
        let editor = ctx.add_typed_action_view(move |ctx| {
            let view = CodeEditorView::new(
                None,
                None,
                CodeEditorRenderOptions::new(VerticalExpansionBehavior::InfiniteHeight),
                ctx,
            );
            view.reset(InitialBufferState::plain_text(content), ctx);
            view.set_interaction_state(InteractionState::Editable, ctx);
            view
        });
        // The raw fallback is the file's JSON, so highlight it as JSON.
        editor.update(ctx, |view, ctx| view.set_language_with_name("json", ctx));
        ctx.subscribe_to_view(&editor, move |me, handle, event, ctx| {
            me.handle_raw_editor_event(handle, event, ctx);
        });
        editor
    }

    /// Build the read-only output elements for a code cell. Image outputs are
    /// decoded and registered with the asset cache here (display-only; the
    /// model's `outputs` are never touched — invariant 17).
    fn build_outputs(
        id: CellId,
        outputs: &[Value],
        position_id: &str,
        ctx: &mut ViewContext<Self>,
    ) -> Vec<RenderableOutput> {
        let mut result = Vec::new();
        for (index, item) in output::classify_outputs(outputs).into_iter().enumerate() {
            match item {
                OutputItem::Text(text) | OutputItem::Placeholder(text) => {
                    result.push(RenderableOutput::Text(text))
                }
                OutputItem::Image(bytes) => {
                    let asset_id = format!("{position_id}_out_{}_{index}", id.0);
                    AssetCache::handle(ctx).update(ctx, |cache, ctx| {
                        cache.insert_raw_asset_bytes::<ImageType>(asset_id.clone(), &bytes, ctx);
                    });
                    result.push(RenderableOutput::Image(AssetSource::Raw { id: asset_id }));
                }
            }
        }
        result
    }

    // --- Edit sync -------------------------------------------------------

    fn handle_markdown_cell_event(
        &mut self,
        id: CellId,
        handle: ViewHandle<RichTextEditorView>,
        event: &EditorViewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorViewEvent::Edited => {
                let markdown = handle.as_ref(ctx).markdown(ctx);
                self.sync_cell_source(id, markdown, ctx);
            }
            EditorViewEvent::Focused => self.emit_focused(ctx),
            // Navigation, selection, menu, and link events don't affect cell content.
            _ => {}
        }
    }

    fn handle_code_cell_event(
        &mut self,
        id: CellId,
        handle: ViewHandle<CodeEditorView>,
        event: &CodeEditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            // Only user-originated edits update the model and mark dirty; the
            // initial content load (and other system edits) use a non-user
            // origin and must not dirty the notebook (invariants 6, 9).
            CodeEditorEvent::ContentChanged { origin } if origin.from_user() => {
                let text = handle.as_ref(ctx).text(ctx).into_string();
                self.sync_cell_source(id, text, ctx);
            }
            CodeEditorEvent::Focused => self.emit_focused(ctx),
            // Other code-editor events (diff, selection, viewport, system edits, ...) are irrelevant here.
            _ => {}
        }
    }

    fn handle_raw_editor_event(
        &mut self,
        handle: ViewHandle<CodeEditorView>,
        event: &CodeEditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            // As with cells, ignore the initial load / system edits; only a user
            // edit of the raw text marks the buffer dirty.
            CodeEditorEvent::ContentChanged { origin } if origin.from_user() => {
                let text = handle.as_ref(ctx).text(ctx).into_string();
                let changed = if let NotebookState::Raw { text: stored, .. } = &mut self.state {
                    *stored = text;
                    true
                } else {
                    false
                };
                if changed {
                    self.mark_dirty(ctx);
                }
            }
            CodeEditorEvent::Focused => self.emit_focused(ctx),
            _ => {}
        }
    }

    /// Write `source` back into the model cell identified by `id`, marking the
    /// notebook dirty. Untouched cells and saved outputs are never re-serialized
    /// (invariants 6, 9).
    fn sync_cell_source(&mut self, id: CellId, source: String, ctx: &mut ViewContext<Self>) {
        let updated = if let NotebookState::Notebook { doc, cells } = &mut self.state {
            if let Some(index) = cells.iter().position(|cell| cell.id == id) {
                doc.cells[index].set_source(&source);
                true
            } else {
                false
            }
        } else {
            false
        };
        if updated {
            self.mark_dirty(ctx);
        }
    }

    fn mark_dirty(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.dirty {
            self.dirty = true;
            ctx.emit(JupyterNotebookEvent::Dirtied);
        }
        ctx.notify();
    }

    fn emit_focused(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.emit(JupyterNotebookEvent::Focused);
        ctx.emit(JupyterNotebookEvent::Pane(PaneEvent::FocusSelf));
    }

    // --- Structural operations (invariant 8) -----------------------------

    fn insert_cell(
        &mut self,
        anchor: Option<CellId>,
        position: InsertPosition,
        kind: CellKind,
        ctx: &mut ViewContext<Self>,
    ) {
        let new_doc_cell = match kind {
            CellKind::Markdown => CellDoc::new_markdown(""),
            CellKind::Code => CellDoc::new_code(""),
            CellKind::Raw => {
                let mut cell = CellDoc::new_markdown("");
                cell.convert_to(CellKind::Raw);
                cell
            }
        };

        let links = self.links.clone();
        let position_id = self.view_position_id.clone();
        let language = self.language.clone();
        let mut next_id = self.next_cell_id;
        let new_view = Self::build_cell_view(
            &new_doc_cell,
            language.as_deref(),
            &links,
            &position_id,
            &mut next_id,
            ctx,
        );
        self.next_cell_id = next_id;

        if let NotebookState::Notebook { doc, cells } = &mut self.state {
            let index = match anchor.and_then(|id| cells.iter().position(|cell| cell.id == id)) {
                Some(anchor_index) => match position {
                    InsertPosition::Above => anchor_index,
                    InsertPosition::Below => anchor_index + 1,
                },
                None => match position {
                    InsertPosition::Above => 0,
                    InsertPosition::Below => cells.len(),
                },
            };
            doc.insert_cell(index, new_doc_cell);
            cells.insert(index.min(cells.len()), new_view);
        }
        self.mark_dirty(ctx);
    }

    fn delete_cell(&mut self, id: CellId, ctx: &mut ViewContext<Self>) {
        let removed = if let NotebookState::Notebook { doc, cells } = &mut self.state {
            if let Some(index) = cells.iter().position(|cell| cell.id == id) {
                doc.remove_cell(index);
                cells.remove(index);
                true
            } else {
                false
            }
        } else {
            false
        };
        if removed {
            self.mark_dirty(ctx);
        }
    }

    fn move_cell(&mut self, id: CellId, down: bool, ctx: &mut ViewContext<Self>) {
        let moved = if let NotebookState::Notebook { doc, cells } = &mut self.state {
            match cells.iter().position(|cell| cell.id == id) {
                Some(index) => {
                    let target = if down {
                        index + 1
                    } else {
                        index.wrapping_sub(1)
                    };
                    if (down && target < cells.len()) || (!down && index > 0) {
                        doc.move_cell(index, target);
                        cells.swap(index, target);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        } else {
            false
        };
        if moved {
            self.mark_dirty(ctx);
        }
    }

    fn convert_cell(&mut self, id: CellId, kind: CellKind, ctx: &mut ViewContext<Self>) {
        let index = match &self.state {
            NotebookState::Notebook { cells, .. } => cells.iter().position(|cell| cell.id == id),
            NotebookState::Raw { .. } => None,
        };
        let Some(index) = index else {
            return;
        };
        // Convert in the model first (drops outputs/execution_count when leaving code).
        if let NotebookState::Notebook { doc, .. } = &mut self.state {
            doc.cells[index].convert_to(kind);
        }

        let links = self.links.clone();
        let position_id = self.view_position_id.clone();
        let language = self.language.clone();
        let mut next_id = self.next_cell_id;
        let new_view = {
            let cell = match &self.state {
                NotebookState::Notebook { doc, .. } => &doc.cells[index],
                NotebookState::Raw { .. } => return,
            };
            Self::build_cell_view(
                cell,
                language.as_deref(),
                &links,
                &position_id,
                &mut next_id,
                ctx,
            )
        };
        self.next_cell_id = next_id;
        if let NotebookState::Notebook { cells, .. } = &mut self.state {
            cells[index] = new_view;
        }
        self.mark_dirty(ctx);
    }

    // --- Rendering -------------------------------------------------------

    fn render_body(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let content = match &self.state {
            NotebookState::Raw { editor, .. } => ChildView::new(editor).finish(),
            NotebookState::Notebook { cells, .. } => self.render_cells(cells, appearance),
        };

        // Vertical-only scroll: the notebook scrolls as a single column while
        // each cell's editor is sized to its content (the cross axis stays
        // constrained to the viewport width).
        let scrollable = NewScrollable::vertical(
            SingleAxisConfig::Clipped {
                handle: self.vertical_scroll_state.clone(),
                child: Container::new(content).with_uniform_padding(12.).finish(),
            },
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            warpui::elements::Fill::None,
        )
        .finish();

        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Expanded::new(1., scrollable).finish())
            .finish()
    }

    fn render_cells(&self, cells: &[CellView], appearance: &Appearance) -> Box<dyn Element> {
        if cells.is_empty() {
            return self.render_empty(appearance);
        }
        let count = cells.len();
        let mut column = Flex::column()
            .with_spacing(12.)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for (index, cell) in cells.iter().enumerate() {
            column.add_child(self.render_cell(index, cell, count, appearance));
        }
        column.finish()
    }

    fn render_cell(
        &self,
        index: usize,
        cell: &CellView,
        cell_count: usize,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let editor_element = match &cell.kind {
            CellViewKind::Markdown(editor) => ChildView::new(editor).finish(),
            CellViewKind::Code(editor) | CellViewKind::Raw(editor) => {
                ChildView::new(editor).finish()
            }
        };

        let mut column = Flex::column()
            .with_spacing(4.)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(self.render_cell_toolbar(index, cell, cell_count, appearance));
        column.add_child(editor_element);
        // Read-only outputs render directly beneath the code editor (invariant 5).
        if !cell.outputs.is_empty() {
            column.add_child(self.render_outputs(&cell.outputs, appearance));
        }

        Container::new(column.finish())
            .with_uniform_padding(8.)
            .with_border(Border::all(1.).with_border_fill(appearance.theme().surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish()
    }

    fn render_cell_toolbar(
        &self,
        index: usize,
        cell: &CellView,
        cell_count: usize,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let id = cell.id;
        let handles = &cell.toolbar;
        let mut row = Flex::row()
            .with_spacing(4.)
            .with_main_axis_size(MainAxisSize::Min);

        if index > 0 {
            row.add_child(toolbar_button(
                &handles.move_up,
                "↑",
                appearance,
                JupyterNotebookAction::MoveCellUp(id),
            ));
        }
        if index + 1 < cell_count {
            row.add_child(toolbar_button(
                &handles.move_down,
                "↓",
                appearance,
                JupyterNotebookAction::MoveCellDown(id),
            ));
        }

        let (target_kind, convert_label) = match &cell.kind {
            CellViewKind::Markdown(_) => (CellKind::Code, "To code"),
            CellViewKind::Code(_) | CellViewKind::Raw(_) => (CellKind::Markdown, "To markdown"),
        };
        row.add_child(toolbar_button(
            &handles.convert,
            convert_label,
            appearance,
            JupyterNotebookAction::ConvertCell {
                id,
                kind: target_kind,
            },
        ));
        row.add_child(toolbar_button(
            &handles.insert_markdown,
            "+md",
            appearance,
            JupyterNotebookAction::InsertCell {
                anchor: Some(id),
                position: InsertPosition::Below,
                kind: CellKind::Markdown,
            },
        ));
        row.add_child(toolbar_button(
            &handles.insert_code,
            "+code",
            appearance,
            JupyterNotebookAction::InsertCell {
                anchor: Some(id),
                position: InsertPosition::Below,
                kind: CellKind::Code,
            },
        ));
        row.add_child(toolbar_button(
            &handles.delete,
            "Delete",
            appearance,
            JupyterNotebookAction::DeleteCell(id),
        ));
        row.finish()
    }

    fn render_outputs(
        &self,
        outputs: &[RenderableOutput],
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_color = theme.main_text_color(theme.surface_2()).into_solid();
        let mut column = Flex::column()
            .with_spacing(4.)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for output in outputs {
            match output {
                RenderableOutput::Text(text) => {
                    let element = Text::new(
                        text.clone(),
                        appearance.monospace_font_family(),
                        OUTPUT_FONT_SIZE,
                    )
                    .with_color(text_color)
                    .finish();
                    column.add_child(
                        Container::new(element)
                            .with_uniform_padding(6.)
                            .with_background(theme.surface_2())
                            .finish(),
                    );
                }
                RenderableOutput::Image(source) => {
                    let image = Image::new(source.clone(), CacheOption::Original)
                        .contain()
                        .layout_using_paint_bounds()
                        .finish();
                    column.add_child(
                        Container::new(image)
                            .with_uniform_padding(6.)
                            .with_background(theme.surface_2())
                            .finish(),
                    );
                }
            }
        }
        column.finish()
    }

    fn render_empty(&self, appearance: &Appearance) -> Box<dyn Element> {
        // A notebook with no cells still renders; the user can add a first cell
        // (invariant 14).
        let mut row = Flex::row()
            .with_spacing(8.)
            .with_main_axis_size(MainAxisSize::Min);
        row.add_child(toolbar_button(
            &self.add_first_markdown,
            "+ Markdown cell",
            appearance,
            JupyterNotebookAction::InsertCell {
                anchor: None,
                position: InsertPosition::Below,
                kind: CellKind::Markdown,
            },
        ));
        row.add_child(toolbar_button(
            &self.add_first_code,
            "+ Code cell",
            appearance,
            JupyterNotebookAction::InsertCell {
                anchor: None,
                position: InsertPosition::Below,
                kind: CellKind::Code,
            },
        ));
        Align::new(row.finish()).finish()
    }
}

/// Build a small toolbar button that dispatches `action` when clicked. The
/// `handle` must be a persistent [`MouseStateHandle`] (created at construction,
/// not inline during render).
fn toolbar_button(
    handle: &MouseStateHandle,
    label: &str,
    appearance: &Appearance,
    action: JupyterNotebookAction,
) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .button(ButtonVariant::Basic, handle.clone())
        .with_text_label(label.to_string())
        .build()
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
}

impl Entity for JupyterNotebookView {
    type Event = JupyterNotebookEvent;
}

impl View for JupyterNotebookView {
    fn ui_name() -> &'static str {
        "JupyterNotebookView"
    }

    fn accessibility_contents(&self, _ctx: &AppContext) -> Option<AccessibilityContent> {
        Some(AccessibilityContent::new_without_help(
            format!("{} notebook", self.title()),
            WarpA11yRole::TextRole,
        ))
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.render_body(app)
    }
}

impl TypedActionView for JupyterNotebookView {
    type Action = JupyterNotebookAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            JupyterNotebookAction::Focus => ctx.focus_self(),
            JupyterNotebookAction::Close => ctx.emit(JupyterNotebookEvent::Pane(PaneEvent::Close)),
            JupyterNotebookAction::Save => self.request_save(ctx),
            JupyterNotebookAction::ToggleMaximized => {
                ctx.emit(JupyterNotebookEvent::Pane(PaneEvent::ToggleMaximized));
                self.pane_configuration.update(ctx, |config, ctx| {
                    config.refresh_pane_header_overflow_menu_items(ctx)
                });
            }
            JupyterNotebookAction::InsertCell {
                anchor,
                position,
                kind,
            } => self.insert_cell(*anchor, *position, *kind, ctx),
            JupyterNotebookAction::DeleteCell(id) => self.delete_cell(*id, ctx),
            JupyterNotebookAction::MoveCellUp(id) => self.move_cell(*id, false, ctx),
            JupyterNotebookAction::MoveCellDown(id) => self.move_cell(*id, true, ctx),
            JupyterNotebookAction::ConvertCell { id, kind } => self.convert_cell(*id, *kind, ctx),
        }
    }
}

impl BackingView for JupyterNotebookView {
    type PaneHeaderOverflowMenuAction = JupyterNotebookAction;
    type CustomAction = ();
    type AssociatedData = ();

    fn handle_pane_header_overflow_menu_action(
        &mut self,
        action: &Self::PaneHeaderOverflowMenuAction,
        ctx: &mut ViewContext<Self>,
    ) {
        self.handle_action(action, ctx);
    }

    fn close(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.emit(JupyterNotebookEvent::Pane(PaneEvent::Close));
    }

    fn focus_contents(&mut self, ctx: &mut ViewContext<Self>) {
        self.focus(ctx);
    }

    fn pane_header_overflow_menu_items(
        &self,
        ctx: &AppContext,
    ) -> Vec<MenuItem<JupyterNotebookAction>> {
        let is_maximized = self
            .focus_handle
            .as_ref()
            .is_some_and(|handle| handle.is_maximized(ctx));
        vec![
            MenuItemFields::toggle_pane_action(is_maximized)
                .with_on_select_action(JupyterNotebookAction::ToggleMaximized)
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new("Save")
                .with_on_select_action(JupyterNotebookAction::Save)
                .into_item(),
        ]
    }

    fn render_header_content(
        &self,
        _ctx: &view::HeaderRenderContext<'_>,
        app: &AppContext,
    ) -> view::HeaderContent {
        let title = self.pane_configuration.as_ref(app).title().to_owned();

        view::HeaderContent::Standard(view::StandardHeader {
            title,
            title_secondary: None,
            title_style: None,
            title_clip_config: warpui::text_layout::ClipConfig::start(),
            title_max_width: None,
            left_of_title: None,
            right_of_title: None,
            left_of_overflow: None,
            options: Default::default(),
        })
    }

    fn set_focus_handle(&mut self, focus_handle: PaneFocusHandle, _ctx: &mut ViewContext<Self>) {
        self.focus_handle = Some(focus_handle);
    }
}

#[cfg(test)]
impl JupyterNotebookView {
    /// Whether the view is in raw-text fallback mode (invariant 16).
    fn is_fallback(&self) -> bool {
        matches!(self.state, NotebookState::Raw { .. })
    }

    /// The number of cells in the parsed notebook (0 in fallback mode).
    fn cell_count(&self) -> usize {
        match &self.state {
            NotebookState::Notebook { cells, .. } => cells.len(),
            NotebookState::Raw { .. } => 0,
        }
    }

    /// The model source text of the cell at `index`, if present.
    fn cell_source(&self, index: usize) -> Option<String> {
        match &self.state {
            NotebookState::Notebook { doc, .. } => {
                doc.cells.get(index).map(|cell| cell.source_text())
            }
            NotebookState::Raw { .. } => None,
        }
    }

    /// The stable id of the cell at `index`, if present.
    fn cell_id_at(&self, index: usize) -> Option<CellId> {
        match &self.state {
            NotebookState::Notebook { cells, .. } => cells.get(index).map(|cell| cell.id),
            NotebookState::Raw { .. } => None,
        }
    }

    /// The kind of the cell at `index`, if present.
    fn cell_kind(&self, index: usize) -> Option<CellKind> {
        match &self.state {
            NotebookState::Notebook { doc, .. } => {
                doc.cells.get(index).and_then(|cell| cell.kind())
            }
            NotebookState::Raw { .. } => None,
        }
    }

    /// The number of read-only outputs rendered beneath the cell at `index`.
    fn output_count(&self, index: usize) -> usize {
        match &self.state {
            NotebookState::Notebook { cells, .. } => {
                cells.get(index).map(|cell| cell.outputs.len()).unwrap_or(0)
            }
            NotebookState::Raw { .. } => 0,
        }
    }

    /// The code editor handle for the cell at `index`, if it is a code/raw cell.
    fn code_editor_at(&self, index: usize) -> Option<ViewHandle<CodeEditorView>> {
        match &self.state {
            NotebookState::Notebook { cells, .. } => match &cells.get(index)?.kind {
                CellViewKind::Code(editor) | CellViewKind::Raw(editor) => Some(editor.clone()),
                CellViewKind::Markdown(_) => None,
            },
            NotebookState::Raw { .. } => None,
        }
    }

    /// The markdown editor handle for the cell at `index`, if it is a markdown cell.
    fn markdown_editor_at(&self, index: usize) -> Option<ViewHandle<RichTextEditorView>> {
        match &self.state {
            NotebookState::Notebook { cells, .. } => match &cells.get(index)?.kind {
                CellViewKind::Markdown(editor) => Some(editor.clone()),
                CellViewKind::Code(_) | CellViewKind::Raw(_) => None,
            },
            NotebookState::Raw { .. } => None,
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

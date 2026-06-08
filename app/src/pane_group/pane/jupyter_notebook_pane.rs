//! Hosting pane for the editable, cell-based Jupyter notebook view.
//!
//! Mirrors [`super::file_pane::FilePane`], but wraps the cell-based
//! [`JupyterNotebookView`] and owns the file I/O around it: a
//! [`JupyterFileHost`] model loads content via [`FileModel`], drives the view
//! on (re)load, persists the view's edits on save, and surfaces save failures
//! and on-disk conflicts. The view itself is edit-only and never touches the
//! filesystem.

use std::path::Path;

#[cfg(feature = "local_fs")]
use warp_files::{FileModel, FileModelEvent};
#[cfg(feature = "local_fs")]
use warp_util::content_version::ContentVersion;
#[cfg(feature = "local_fs")]
use warp_util::file::FileId;
use warp_util::local_or_remote_path::LocalOrRemotePath;
#[cfg(feature = "local_fs")]
use warp_util::remote_path::RemotePath;
#[cfg(feature = "local_fs")]
use warpui::ModelContext;
use warpui::{AppContext, Entity, ModelHandle, SingletonEntity, View, ViewContext, ViewHandle};

use super::view::PaneView;
use super::{
    DetachType, PaneConfiguration, PaneContent, PaneEvent, PaneGroup, PaneId, ShareableLink,
    ShareableLinkError,
};
use crate::app_state::{LeafContents, NotebookPaneSnapshot};
use crate::notebooks::file::jupyter::{JupyterNotebookEvent, JupyterNotebookView};
#[cfg(feature = "local_fs")]
use crate::view_components::ToastFlavor;

/// A pane that hosts an editable Jupyter notebook backed by an `.ipynb` file.
pub struct JupyterNotebookPane {
    view: ViewHandle<PaneView<JupyterNotebookView>>,
    pane_configuration: ModelHandle<PaneConfiguration>,
    /// Owns the file I/O (load / save / conflict) around the view.
    #[cfg(feature = "local_fs")]
    file_host: Option<ModelHandle<JupyterFileHost>>,
}

impl JupyterNotebookPane {
    fn from_view(
        view: ViewHandle<JupyterNotebookView>,
        #[cfg(feature = "local_fs")] file_host: Option<ModelHandle<JupyterFileHost>>,
        ctx: &mut AppContext,
    ) -> Self {
        let pane_configuration = view.as_ref(ctx).pane_configuration();
        let pane_view = ctx.add_typed_action_view(view.window_id(ctx), |ctx| {
            let pane_id = PaneId::from_jupyter_notebook_pane_ctx(ctx);
            PaneView::new(pane_id, view, (), pane_configuration.clone(), ctx)
        });

        Self {
            view: pane_view,
            pane_configuration,
            #[cfg(feature = "local_fs")]
            file_host,
        }
    }

    /// Create a new Jupyter notebook pane for `path`. The view starts empty and
    /// is populated once the file's content loads (local or remote). If `path`
    /// is `None`, the pane is created with an empty, unbacked notebook.
    pub fn new<V: View>(path: Option<LocalOrRemotePath>, ctx: &mut ViewContext<V>) -> Self {
        let view_path = path.clone();
        let view =
            ctx.add_typed_action_view(move |ctx| JupyterNotebookView::new("", view_path, ctx));

        #[cfg(feature = "local_fs")]
        let file_host = path.map(|path| ctx.add_model(|_ctx| JupyterFileHost::new(path)));

        Self::from_view(
            view,
            #[cfg(feature = "local_fs")]
            file_host,
            ctx,
        )
    }

    /// The hosted cell-based notebook view.
    fn jupyter_view(&self, ctx: &AppContext) -> ViewHandle<JupyterNotebookView> {
        self.view.as_ref(ctx).child(ctx)
    }
}

impl PaneContent for JupyterNotebookPane {
    fn id(&self) -> PaneId {
        PaneId::from_jupyter_notebook_pane_view(&self.view)
    }

    fn attach(
        &self,
        _group: &PaneGroup,
        focus_handle: crate::pane_group::focus_state::PaneFocusHandle,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        self.view
            .update(ctx, |view, ctx| view.set_focus_handle(focus_handle, ctx));

        let pane_id = self.id();
        let jupyter_view = self.jupyter_view(ctx);

        // Forward the view's events: save requests drive the file host, the raw
        // toggle swaps to a code pane, and pane events bubble up as usual.
        ctx.subscribe_to_view(&jupyter_view, {
            #[cfg(feature = "local_fs")]
            let file_host = self.file_host.clone();
            move |pane_group, view_handle, event, ctx| match event {
                JupyterNotebookEvent::Pane(pane_event) => {
                    pane_group.handle_pane_event(pane_id, pane_event, ctx);
                }
                JupyterNotebookEvent::Focused => {
                    pane_group.handle_pane_event(pane_id, &PaneEvent::FocusSelf, ctx);
                }
                JupyterNotebookEvent::Dirtied => {
                    #[cfg(feature = "local_fs")]
                    if let Some(host) = &file_host {
                        host.update(ctx, |host, _ctx| host.set_dirty());
                    }
                    // Persist the dirty transition into session restore state.
                    pane_group.handle_pane_event(pane_id, &PaneEvent::AppStateChanged, ctx);
                }
                JupyterNotebookEvent::SaveRequested { json } => {
                    #[cfg(feature = "local_fs")]
                    if let Some(host) = &file_host {
                        let json = json.clone();
                        host.update(ctx, |host, ctx| host.save(json, ctx));
                    }
                    #[cfg(not(feature = "local_fs"))]
                    let _ = json;
                }
                JupyterNotebookEvent::RawRequested { json } => {
                    // Emitted only when the file is not a parseable v4 notebook
                    // (invariant 16): swap this pane for a real code editor on the
                    // same file so it stays editable as plain text. This fires at
                    // load time, before any edits, so the on-disk content already
                    // matches the view and `json` is not needed here.
                    let _ = json;
                    #[cfg(feature = "local_fs")]
                    if let Some(path) = view_handle.as_ref(ctx).path().cloned() {
                        pane_group.handle_pane_event(
                            pane_id,
                            &PaneEvent::ReplaceWithCodePane { path, source: None },
                            ctx,
                        );
                    }
                    #[cfg(not(feature = "local_fs"))]
                    let _ = view_handle;
                }
            }
        });

        // Drive the view from the file host's load/save/conflict events.
        #[cfg(feature = "local_fs")]
        if let Some(host) = &self.file_host {
            let view = jupyter_view.clone();
            ctx.subscribe_to_model(host, move |_pane_group, _, event, ctx| match event {
                JupyterFileHostEvent::Loaded { content } => {
                    view.update(ctx, |view, ctx| view.set_content(content, ctx));
                }
                JupyterFileHostEvent::Saved => {
                    view.update(ctx, |view, ctx| view.mark_saved(ctx));
                }
                JupyterFileHostEvent::LoadFailed => {
                    ctx.emit(crate::pane_group::Event::ShowToast {
                        message: "Unable to read notebook file".to_owned(),
                        flavor: ToastFlavor::Error,
                        pane_id: Some(pane_id),
                    });
                }
                JupyterFileHostEvent::SaveFailed { message } => {
                    ctx.emit(crate::pane_group::Event::ShowToast {
                        message: format!("Failed to save notebook: {message}"),
                        flavor: ToastFlavor::Error,
                        pane_id: Some(pane_id),
                    });
                }
                JupyterFileHostEvent::Conflict => {
                    // Do not overwrite the user's unsaved edits. Inform them; they
                    // can save to overwrite or close/reopen to take the disk copy.
                    ctx.emit(crate::pane_group::Event::ShowToast {
                        message: "Notebook changed on disk. Your unsaved edits are preserved; \
                                  saving will overwrite the on-disk version."
                            .to_owned(),
                        flavor: ToastFlavor::Default,
                        pane_id: Some(pane_id),
                    });
                }
            });

            // Start loading only after the subscriptions above are registered so
            // the initial load event is not missed.
            host.update(ctx, |host, ctx| host.start_if_needed(ctx));
        }

        ctx.subscribe_to_view(&self.view, move |group, _, event, ctx| {
            group.handle_pane_view_event(pane_id, event, ctx);
        });
    }

    fn detach(
        &self,
        _group: &PaneGroup,
        _detach_type: DetachType,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        let jupyter_view = self.jupyter_view(ctx);
        ctx.unsubscribe_to_view(&jupyter_view);
        #[cfg(feature = "local_fs")]
        if let Some(host) = &self.file_host {
            ctx.unsubscribe_to_model(host);
        }
        ctx.unsubscribe_to_view(&self.view);
    }

    fn snapshot(&self, app: &AppContext) -> LeafContents {
        // Only local files are restorable across sessions. The path-keyed
        // restore arm re-routes `.ipynb` back to this pane when the flag is on.
        let path = self
            .jupyter_view(app)
            .as_ref(app)
            .path()
            .and_then(|p| p.to_local_path().map(Path::to_path_buf));
        LeafContents::Notebook(NotebookPaneSnapshot::LocalFileNotebook { path })
    }

    fn has_application_focus(&self, ctx: &mut ViewContext<PaneGroup>) -> bool {
        self.view.is_self_or_child_focused(ctx)
    }

    fn focus(&self, ctx: &mut ViewContext<PaneGroup>) {
        let view = self.jupyter_view(ctx);
        ctx.focus(&view);
    }

    fn shareable_link(
        &self,
        _ctx: &mut ViewContext<PaneGroup>,
    ) -> Result<ShareableLink, ShareableLinkError> {
        Ok(ShareableLink::Base)
    }

    fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    fn is_pane_being_dragged(&self, ctx: &AppContext) -> bool {
        self.view.as_ref(ctx).is_being_dragged()
    }
}

/// Events emitted by [`JupyterFileHost`] to drive the hosted view.
#[cfg(feature = "local_fs")]
#[derive(Debug, Clone)]
enum JupyterFileHostEvent {
    /// Fresh content was loaded from disk (initial load or external change while
    /// the buffer is clean).
    Loaded { content: String },
    /// The file could not be read.
    LoadFailed,
    /// A save completed successfully.
    Saved,
    /// A save failed; `message` is a user-facing description.
    SaveFailed { message: String },
    /// The file changed on disk while the buffer had unsaved edits.
    Conflict,
}

/// Owns the file-backing for a [`JupyterNotebookPane`]: it loads content via
/// [`FileModel`], tracks the [`ContentVersion`] and dirty state, persists the
/// view's edits, and detects on-disk conflicts.
#[cfg(feature = "local_fs")]
struct JupyterFileHost {
    path: LocalOrRemotePath,
    file_id: Option<FileId>,
    /// Version of the content currently reflected on disk and in the view.
    version: Option<ContentVersion>,
    /// Whether the view has unsaved edits (mirrors the view's dirty state).
    dirty: bool,
    /// Whether [`Self::start_if_needed`] has already kicked off a load.
    started: bool,
}

#[cfg(feature = "local_fs")]
impl Entity for JupyterFileHost {
    type Event = JupyterFileHostEvent;
}

#[cfg(feature = "local_fs")]
impl JupyterFileHost {
    fn new(path: LocalOrRemotePath) -> Self {
        Self {
            path,
            file_id: None,
            version: None,
            dirty: false,
            started: false,
        }
    }

    /// Begin loading the file (idempotent). Local files are read and watched via
    /// [`FileModel`]; remote files are fetched from the remote server and
    /// registered with [`FileModel`] for save dispatch.
    fn start_if_needed(&mut self, ctx: &mut ModelContext<Self>) {
        if self.started {
            return;
        }
        self.started = true;

        match self.path.clone() {
            LocalOrRemotePath::Local(local_path) => {
                let file_model = FileModel::handle(ctx);
                let file_id =
                    file_model.update(ctx, |model, ctx| model.open(&local_path, true, ctx));
                self.file_id = Some(file_id);
                ctx.subscribe_to_model(&file_model, move |me, event: &FileModelEvent, ctx| {
                    if event.file_id() == file_id {
                        me.handle_file_event(event, ctx);
                    }
                });
            }
            LocalOrRemotePath::Remote(remote_path) => {
                let file_model = FileModel::handle(ctx);
                let file_id = file_model.update(ctx, |model, _ctx| {
                    model
                        .register_remote_file(remote_path.host_id.clone(), remote_path.path.clone())
                });
                self.file_id = Some(file_id);
                ctx.subscribe_to_model(&file_model, move |me, event: &FileModelEvent, ctx| {
                    if event.file_id() == file_id {
                        me.handle_file_event(event, ctx);
                    }
                });
                self.fetch_remote(remote_path, ctx);
            }
        }
    }

    /// Mark the buffer dirty so a concurrent on-disk change is treated as a
    /// conflict rather than silently overwriting the user's edits.
    fn set_dirty(&mut self) {
        self.dirty = true;
    }

    /// Persist `json` to the backing file, tracking a fresh [`ContentVersion`].
    fn save(&mut self, json: String, ctx: &mut ModelContext<Self>) {
        let Some(file_id) = self.file_id else {
            return;
        };
        let version = ContentVersion::new();
        let result = FileModel::handle(ctx)
            .update(ctx, |model, ctx| model.save(file_id, json, version, ctx));
        if let Err(err) = result {
            ctx.emit(JupyterFileHostEvent::SaveFailed {
                message: err.to_string(),
            });
        }
    }

    fn handle_file_event(&mut self, event: &FileModelEvent, ctx: &mut ModelContext<Self>) {
        match event {
            FileModelEvent::FileLoaded {
                content, version, ..
            } => {
                self.version = Some(*version);
                self.dirty = false;
                ctx.emit(JupyterFileHostEvent::Loaded {
                    content: content.clone(),
                });
            }
            FileModelEvent::FailedToLoad { .. } => {
                ctx.emit(JupyterFileHostEvent::LoadFailed);
            }
            FileModelEvent::FileUpdated {
                content,
                new_version,
                ..
            } => {
                if self.dirty {
                    // Unsaved edits present: never silently overwrite them.
                    ctx.emit(JupyterFileHostEvent::Conflict);
                } else {
                    self.version = Some(*new_version);
                    ctx.emit(JupyterFileHostEvent::Loaded {
                        content: content.clone(),
                    });
                }
            }
            FileModelEvent::FileSaved { version, .. } => {
                // Advance the tracked version so the watcher echo of our own
                // write is not mistaken for an external conflict.
                self.version = Some(*version);
                self.dirty = false;
                ctx.emit(JupyterFileHostEvent::Saved);
            }
            FileModelEvent::FailedToSave { error, .. } => {
                ctx.emit(JupyterFileHostEvent::SaveFailed {
                    message: error.to_string(),
                });
            }
        }
    }

    /// Fetch the content of a remote file via the remote server.
    fn fetch_remote(&mut self, remote_path: RemotePath, ctx: &mut ModelContext<Self>) {
        let host_id = remote_path.host_id.clone();
        let request = remote_server::proto::ReadFileContextRequest {
            files: vec![remote_server::proto::ReadFileContextFile {
                path: remote_path.path.as_str().to_string(),
                line_ranges: vec![],
            }],
            max_file_bytes: None,
            max_batch_bytes: None,
        };
        let handle =
            remote_server::manager::RemoteServerManager::as_ref(ctx).host_request_handle(&host_id);
        ctx.spawn(
            async move { handle.read_file_context(request).await },
            move |me, result, ctx| match result {
                Ok(response) => {
                    if let Some(file_ctx) = response.file_contexts.first() {
                        let text = match &file_ctx.content {
                            Some(
                                remote_server::proto::file_context_proto::Content::TextContent(
                                    text,
                                ),
                            ) => text.clone(),
                            _ => String::new(),
                        };
                        me.version = Some(ContentVersion::new());
                        me.dirty = false;
                        ctx.emit(JupyterFileHostEvent::Loaded { content: text });
                    } else {
                        ctx.emit(JupyterFileHostEvent::LoadFailed);
                    }
                }
                Err(_) => ctx.emit(JupyterFileHostEvent::LoadFailed),
            },
        );
    }
}

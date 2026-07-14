//! Stateful inline Markdown card for CreateDocuments and EditDocuments tool calls.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ai::agent::action_result::{
    AIAgentActionResultType, CreateDocumentsResult, EditDocumentsResult,
};
use markdown_parser::{parse_markdown_with_gfm_tables, FormattedText, FormattedTextLine};
use warp::tui_export::{
    AIAgentAction, AIAgentActionType, BlocklistAIActionEvent, BlocklistAIActionModel,
};
use warpui_core::elements::tui::{
    tui_collapsible, Modifier, TuiChildView, TuiContainer, TuiElement, TuiFlex, TuiParentElement,
    TuiText,
};
use warpui_core::elements::{CrossAxisAlignment, MouseStateHandle};
use warpui_core::{
    AppContext, Entity, EntityId, ModelHandle, TuiView, TypedActionView, ViewContext, ViewHandle,
};

use crate::agent_block_sections::{tool_call_glyph_style, tool_call_label_style};
use crate::tool_call_labels::{tool_call_display_state, tool_call_glyph, ToolCallDisplayState};
use crate::tui_builder::TuiUiBuilder;
use crate::tui_code_block_view::{TuiCodeBlockPayload, TuiCodeBlockView, TuiCodeBlockViewEvent};
use crate::tui_markdown::{render_formatted_text, TuiMarkdownBlockHooks, TuiMarkdownPalette};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TuiPlanCodeKey {
    document_index: usize,
    code_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedPlanDocument {
    title: String,
    content: String,
}

struct TuiPlanDocument {
    source: ResolvedPlanDocument,
    formatted: Option<Arc<FormattedText>>,
}

pub(super) enum TuiPlanViewEvent {
    LayoutChanged,
}

#[derive(Clone, Debug)]
pub(super) enum TuiPlanViewAction {
    SetCollapsed(bool),
}

pub(super) struct TuiPlanView {
    action: AIAgentAction,
    action_model: ModelHandle<BlocklistAIActionModel>,
    output_streaming: bool,
    documents: Vec<TuiPlanDocument>,
    code_views: HashMap<TuiPlanCodeKey, ViewHandle<TuiCodeBlockView>>,
    collapsed: bool,
    header_mouse_state: MouseStateHandle,
}

impl TuiPlanView {
    pub(super) fn new(
        action: AIAgentAction,
        output_streaming: bool,
        action_model: &ModelHandle<BlocklistAIActionModel>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let mut view = Self {
            action,
            action_model: action_model.clone(),
            output_streaming,
            documents: Vec::new(),
            code_views: HashMap::new(),
            collapsed: false,
            header_mouse_state: MouseStateHandle::default(),
        };
        view.sync_documents(ctx);

        ctx.subscribe_to_model(action_model, |me, _, event, ctx| {
            if matches!(
                event,
                BlocklistAIActionEvent::FinishedAction { action_id, .. }
                    if *action_id == me.action.id
            ) {
                me.sync_documents(ctx);
                me.invalidate_layout(ctx);
            }
        });
        view
    }

    pub(super) fn sync_action(
        &mut self,
        action: AIAgentAction,
        output_streaming: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let status_changed = self.output_streaming != output_streaming;
        let action_changed = self.action != action;
        self.action = action;
        self.output_streaming = output_streaming;
        let documents_changed = self.sync_documents(ctx);
        if status_changed || action_changed || documents_changed {
            self.invalidate_layout(ctx);
        }
    }

    pub(super) fn renders_rich_body(&self) -> bool {
        !self.documents.is_empty()
    }

    fn sync_documents(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let resolved = self.resolve_documents(ctx);
        if self
            .documents
            .iter()
            .map(|document| &document.source)
            .eq(resolved.iter())
        {
            return false;
        }

        self.documents = resolved
            .into_iter()
            .map(|source| TuiPlanDocument {
                formatted: parse_markdown_with_gfm_tables(&source.content)
                    .ok()
                    .map(Arc::new),
                source,
            })
            .collect();
        self.sync_code_views(ctx);
        true
    }

    fn resolve_documents(&self, app: &AppContext) -> Vec<ResolvedPlanDocument> {
        let result = self
            .action_model
            .as_ref(app)
            .get_action_result(&self.action.id)
            .map(Arc::as_ref);
        match (&self.action.action, result) {
            (AIAgentActionType::CreateDocuments(request), Some(result)) => {
                let AIAgentActionResultType::CreateDocuments(CreateDocumentsResult::Success {
                    created_documents,
                }) = &result.result
                else {
                    return Vec::new();
                };
                created_documents
                    .iter()
                    .enumerate()
                    .map(|(index, document)| ResolvedPlanDocument {
                        title: request
                            .documents
                            .get(index)
                            .map(|document| document.title.clone())
                            .filter(|title| !title.is_empty())
                            .unwrap_or_else(|| document_title(index, created_documents.len())),
                        content: document.content.clone(),
                    })
                    .collect()
            }
            (AIAgentActionType::EditDocuments(_), Some(result)) => {
                let AIAgentActionResultType::EditDocuments(EditDocumentsResult::Success {
                    updated_documents,
                }) = &result.result
                else {
                    return Vec::new();
                };
                updated_documents
                    .iter()
                    .enumerate()
                    .map(|(index, document)| ResolvedPlanDocument {
                        title: document_title(index, updated_documents.len()),
                        content: document.content.clone(),
                    })
                    .collect()
            }
            (AIAgentActionType::CreateDocuments(request), None) => request
                .documents
                .iter()
                .map(|document| ResolvedPlanDocument {
                    title: if document.title.is_empty() {
                        "Plan".to_owned()
                    } else {
                        document.title.clone()
                    },
                    content: document.content.clone(),
                })
                .collect(),
            (AIAgentActionType::EditDocuments(_), None) => Vec::new(),
            (
                AIAgentActionType::RequestCommandOutput { .. }
                | AIAgentActionType::WriteToLongRunningShellCommand { .. }
                | AIAgentActionType::ReadFiles(_)
                | AIAgentActionType::UploadArtifact(_)
                | AIAgentActionType::SearchCodebase(_)
                | AIAgentActionType::RequestFileEdits { .. }
                | AIAgentActionType::Grep { .. }
                | AIAgentActionType::FileGlob { .. }
                | AIAgentActionType::FileGlobV2 { .. }
                | AIAgentActionType::ReadMCPResource { .. }
                | AIAgentActionType::CallMCPTool { .. }
                | AIAgentActionType::SuggestNewConversation { .. }
                | AIAgentActionType::SuggestPrompt(_)
                | AIAgentActionType::InitProject
                | AIAgentActionType::OpenCodeReview
                | AIAgentActionType::ReadDocuments(_)
                | AIAgentActionType::ReadShellCommandOutput { .. }
                | AIAgentActionType::UseComputer(_)
                | AIAgentActionType::InsertCodeReviewComments { .. }
                | AIAgentActionType::RequestComputerUse(_)
                | AIAgentActionType::StartRecording { .. }
                | AIAgentActionType::StopRecording { .. }
                | AIAgentActionType::ReadSkill(_)
                | AIAgentActionType::FetchConversation { .. }
                | AIAgentActionType::StartAgent { .. }
                | AIAgentActionType::SendMessageToAgent { .. }
                | AIAgentActionType::TransferShellCommandControlToUser { .. }
                | AIAgentActionType::AskUserQuestion { .. }
                | AIAgentActionType::RunAgents(_)
                | AIAgentActionType::WaitForEvents { .. },
                _,
            ) => Vec::new(),
        }
    }

    fn sync_code_views(&mut self, ctx: &mut ViewContext<Self>) {
        let mut descriptors = Vec::new();
        for (document_index, document) in self.documents.iter().enumerate() {
            let Some(formatted) = &document.formatted else {
                continue;
            };
            let mut code_index = 0;
            for line in &formatted.lines {
                if let FormattedTextLine::CodeBlock(code) = line {
                    descriptors.push((
                        TuiPlanCodeKey {
                            document_index,
                            code_index,
                        },
                        TuiCodeBlockPayload::new(
                            code.code.clone(),
                            (!code.lang.is_empty()).then(|| code.lang.clone()),
                        ),
                    ));
                    code_index += 1;
                }
            }
        }

        let active_keys = descriptors
            .iter()
            .map(|(key, _)| *key)
            .collect::<HashSet<_>>();
        self.code_views.retain(|key, _| active_keys.contains(key));

        for (key, payload) in descriptors {
            if let Some(view) = self.code_views.get(&key) {
                view.update(ctx, |view, ctx| {
                    view.sync(payload, ctx);
                });
                continue;
            }
            let view = ctx.add_tui_view(move |ctx| TuiCodeBlockView::new(payload, ctx));
            ctx.subscribe_to_view(&view, |me, _, event, ctx| match event {
                TuiCodeBlockViewEvent::LayoutChanged | TuiCodeBlockViewEvent::SyntaxUpdated => {
                    me.invalidate_layout(ctx)
                }
            });
            self.code_views.insert(key, view);
        }
    }

    fn display_state(&self, app: &AppContext) -> ToolCallDisplayState {
        let status = self
            .action_model
            .as_ref(app)
            .get_action_status(&self.action.id);
        tool_call_display_state(status.as_ref(), self.output_streaming, None)
    }

    fn header_label(&self, state: ToolCallDisplayState) -> String {
        let subject = if self.documents.len() == 1 {
            self.documents[0].source.title.clone()
        } else {
            format!("{} documents", self.documents.len())
        };
        let verb = match (&self.action.action, state) {
            (
                AIAgentActionType::CreateDocuments(_),
                ToolCallDisplayState::Constructing | ToolCallDisplayState::Running,
            ) => "Creating",
            (
                AIAgentActionType::CreateDocuments(_),
                ToolCallDisplayState::Pending | ToolCallDisplayState::AwaitingApproval,
            ) => "Create",
            (AIAgentActionType::CreateDocuments(_), ToolCallDisplayState::Succeeded) => "Created",
            (
                AIAgentActionType::CreateDocuments(_),
                ToolCallDisplayState::Failed | ToolCallDisplayState::Cancelled,
            ) => "Create",
            (
                AIAgentActionType::EditDocuments(_),
                ToolCallDisplayState::Constructing | ToolCallDisplayState::Running,
            ) => "Updating",
            (
                AIAgentActionType::EditDocuments(_),
                ToolCallDisplayState::Pending | ToolCallDisplayState::AwaitingApproval,
            ) => "Update",
            (AIAgentActionType::EditDocuments(_), ToolCallDisplayState::Succeeded) => "Updated",
            (
                AIAgentActionType::EditDocuments(_),
                ToolCallDisplayState::Failed | ToolCallDisplayState::Cancelled,
            ) => "Update",
            (
                AIAgentActionType::RequestCommandOutput { .. }
                | AIAgentActionType::WriteToLongRunningShellCommand { .. }
                | AIAgentActionType::ReadFiles(_)
                | AIAgentActionType::UploadArtifact(_)
                | AIAgentActionType::SearchCodebase(_)
                | AIAgentActionType::RequestFileEdits { .. }
                | AIAgentActionType::Grep { .. }
                | AIAgentActionType::FileGlob { .. }
                | AIAgentActionType::FileGlobV2 { .. }
                | AIAgentActionType::ReadMCPResource { .. }
                | AIAgentActionType::CallMCPTool { .. }
                | AIAgentActionType::SuggestNewConversation { .. }
                | AIAgentActionType::SuggestPrompt(_)
                | AIAgentActionType::InitProject
                | AIAgentActionType::OpenCodeReview
                | AIAgentActionType::ReadDocuments(_)
                | AIAgentActionType::ReadShellCommandOutput { .. }
                | AIAgentActionType::UseComputer(_)
                | AIAgentActionType::InsertCodeReviewComments { .. }
                | AIAgentActionType::RequestComputerUse(_)
                | AIAgentActionType::StartRecording { .. }
                | AIAgentActionType::StopRecording { .. }
                | AIAgentActionType::ReadSkill(_)
                | AIAgentActionType::FetchConversation { .. }
                | AIAgentActionType::StartAgent { .. }
                | AIAgentActionType::SendMessageToAgent { .. }
                | AIAgentActionType::TransferShellCommandControlToUser { .. }
                | AIAgentActionType::AskUserQuestion { .. }
                | AIAgentActionType::RunAgents(_)
                | AIAgentActionType::WaitForEvents { .. },
                _,
            ) => "Plan",
        };
        format!("{verb} {subject}")
    }

    fn render_documents(&self, app: &AppContext) -> Box<dyn TuiElement> {
        let builder = TuiUiBuilder::from_app(app);
        let palette = TuiMarkdownPalette::from_builder(&builder);
        let mut documents =
            TuiFlex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for (document_index, document) in self.documents.iter().enumerate() {
            if document_index > 0 {
                documents.add_child(TuiText::new(" ").truncate().finish());
            }
            if self.documents.len() > 1 {
                documents.add_child(
                    TuiText::new(document.source.title.clone())
                        .with_style(builder.primary_text_style().add_modifier(Modifier::BOLD))
                        .finish(),
                );
            }
            let content = match &document.formatted {
                Some(formatted) => {
                    let render_code =
                        |code_index: usize, _code: &markdown_parser::CodeBlockText| {
                            self.code_views
                                .get(&TuiPlanCodeKey {
                                    document_index,
                                    code_index,
                                })
                                .map(|view| TuiChildView::new(view).finish())
                        };
                    render_formatted_text(
                        formatted,
                        palette,
                        &TuiMarkdownBlockHooks {
                            render_code: Some(&render_code),
                        },
                    )
                }
                None => TuiText::new(document.source.content.clone())
                    .with_style(palette.body)
                    .finish(),
            };
            documents.add_child(content);
        }

        TuiContainer::new(documents.finish())
            .with_padding_x(2)
            .with_padding_y(1)
            .with_background(builder.plan_background())
            .finish()
    }

    fn invalidate_layout(&self, ctx: &mut ViewContext<Self>) {
        ctx.emit(TuiPlanViewEvent::LayoutChanged);
        ctx.notify();
    }
}

fn document_title(index: usize, document_count: usize) -> String {
    if document_count == 1 {
        "Plan".to_owned()
    } else {
        format!("Document {}", index + 1)
    }
}

impl Entity for TuiPlanView {
    type Event = TuiPlanViewEvent;
}

impl TuiView for TuiPlanView {
    fn ui_name() -> &'static str {
        "TuiPlanView"
    }

    fn child_view_ids(&self, _app: &AppContext) -> Vec<EntityId> {
        self.code_views.values().map(|view| view.id()).collect()
    }

    fn render(&self, app: &AppContext) -> Box<dyn TuiElement> {
        let builder = TuiUiBuilder::from_app(app);
        let state = self.display_state(app);
        let glyph_style = tool_call_glyph_style(state, &builder);
        let label_style = tool_call_label_style(state, &builder).add_modifier(Modifier::BOLD);
        let label_style = if self.header_mouse_state.lock().unwrap().is_hovered() {
            label_style.add_modifier(Modifier::UNDERLINED)
        } else {
            label_style
        };
        let line_count = self
            .documents
            .iter()
            .map(|document| document.source.content.lines().count())
            .sum::<usize>();
        let mut header = vec![
            (format!("{} ", tool_call_glyph(state)), glyph_style),
            (self.header_label(state), label_style),
        ];
        if line_count > 0 {
            header.push((
                format!(" +{line_count}"),
                builder.diff_added_style().add_modifier(Modifier::BOLD),
            ));
        }
        let collapsed = self.collapsed;
        tui_collapsible(
            collapsed,
            header,
            builder.primary_text_style(),
            self.header_mouse_state.clone(),
            || self.render_documents(app),
            move |event_ctx, _app| {
                event_ctx.dispatch_typed_action(TuiPlanViewAction::SetCollapsed(!collapsed));
            },
        )
    }
}

impl TypedActionView for TuiPlanView {
    type Action = TuiPlanViewAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            TuiPlanViewAction::SetCollapsed(collapsed) => {
                self.collapsed = *collapsed;
                self.invalidate_layout(ctx);
            }
        }
    }
}

#[cfg(test)]
#[path = "tui_plan_view_tests.rs"]
mod tests;

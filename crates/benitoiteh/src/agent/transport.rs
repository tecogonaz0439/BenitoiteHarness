//! アダプタの要求とストリームの違いをループから隠す（設計書 02-01「層の分け方」）。

use std::time::Duration;

use crate::context::SystemContext;
use crate::http::{HttpClient, HttpError};
use crate::provider::anthropic::{AnthropicAdapter, AnthropicRequest, AnthropicStream};
use crate::provider::openai_chat::{OpenAiChatAdapter, OpenAiChatRequest, OpenAiChatStream};
use crate::provider::openai_responses::{
    OpenAiResponsesAdapter, OpenAiResponsesRequest, OpenAiResponsesStream,
};
use crate::provider::{
    AdapterKind, AssistantTurn, Conversation, NonRetryableKind, ProviderError, StreamEvent,
};

use super::{Agent, AgentConfig, AgentError};
use crate::provider::openai_responses::ChatGptTokens;

impl Agent {
    /// 最後の応答の形を決める JSON Schema を、普通の番の要求に付ける（設計書 05-01「exec の出力」）。
    pub fn with_output_schema(mut self, schema: Option<serde_json::Value>) -> Self {
        self.output_schema = schema;
        self
    }

    /// 起動時に検査済みの ChatGPT の保存を Responses のアダプタに渡す。
    pub fn with_chatgpt_tokens(mut self, source: ChatGptTokens) -> Result<Self, AgentError> {
        let Adapter::Responses(adapter) = self.adapter else {
            return Err(AgentError::InvalidConfiguration);
        };
        self.adapter = Adapter::Responses(adapter.with_chatgpt_tokens(source));
        Ok(self)
    }
}

pub(super) enum Adapter {
    Anthropic(AnthropicAdapter),
    Chat(OpenAiChatAdapter),
    Responses(OpenAiResponsesAdapter),
}

pub(super) enum Request {
    Anthropic(AnthropicRequest),
    Chat(OpenAiChatRequest),
    Responses(OpenAiResponsesRequest),
}

impl Request {
    pub(super) fn preview(&self) -> crate::provider::RequestPreview {
        match self {
            Self::Anthropic(request) => request.preview(),
            Self::Chat(request) => request.preview(),
            Self::Responses(request) => request.preview(),
        }
    }
}

impl Agent {
    /// 次の依頼で最初に送る要求を、送らずに作る（設計書 05-01「debug prompt-input」）。
    ///
    /// 会話と記録は変えない。依頼の前に置く環境の部分は、`run_request` と同じ規則で加える。
    pub fn preview_request(
        &self,
        request: String,
    ) -> Result<crate::provider::RequestPreview, AgentError> {
        let mut conversation = self.conversation.clone();
        let message = self.request_message(self.environment_changes.clone(), request);
        conversation
            .items
            .push(crate::provider::Item::UserMessage(message));
        let request = self.adapter.request(
            &conversation,
            &self.system,
            &self.config.effort,
            self.previous_last_item,
            &self.session.id().to_string(),
            self.output_schema.as_ref(),
        )?;
        Ok(request.preview())
    }
}

pub(super) enum Stream {
    Anthropic(AnthropicStream),
    Chat(OpenAiChatStream),
    Responses(OpenAiResponsesStream),
}

impl Adapter {
    pub(super) fn set_model(&mut self, model: &str) {
        match self {
            Self::Anthropic(adapter) => adapter.set_model(model),
            Self::Chat(adapter) => adapter.set_model(model),
            Self::Responses(adapter) => adapter.set_model(model),
        }
    }

    pub(super) fn new(config: &AgentConfig, key: String) -> Result<Self, ProviderError> {
        Ok(match config.kind {
            AdapterKind::AnthropicMessages => Self::Anthropic(AnthropicAdapter::new(
                config.destination.clone(),
                config.capabilities.clone(),
                key,
            )),
            AdapterKind::OpenAiChat => Self::Chat(OpenAiChatAdapter::new(
                config.destination.clone(),
                config.capabilities.clone(),
                key,
            )),
            AdapterKind::OpenAiResponses => Self::Responses(OpenAiResponsesAdapter::new(
                config.destination.clone(),
                config.capabilities.clone(),
                key,
            )),
        })
    }

    pub(super) fn request(
        &self,
        conversation: &Conversation,
        system: &SystemContext,
        effort: &str,
        previous: Option<usize>,
        session_id: &str,
        output_schema: Option<&serde_json::Value>,
    ) -> Result<Request, ProviderError> {
        self.build(
            conversation,
            system,
            effort,
            previous,
            session_id,
            (false, output_schema),
        )
    }

    pub(super) fn request_with(
        &self,
        conversation: &Conversation,
        system: &SystemContext,
        effort: &str,
        previous: Option<usize>,
        session_id: &str,
        disable_tools: bool,
    ) -> Result<Request, ProviderError> {
        self.build(
            conversation,
            system,
            effort,
            previous,
            session_id,
            (disable_tools, None),
        )
    }

    /// 要求を組み立てる。最後の応答の形の指定は、普通の番にだけ付ける。
    fn build(
        &self,
        conversation: &Conversation,
        system: &SystemContext,
        effort: &str,
        previous: Option<usize>,
        session_id: &str,
        (disable_tools, output_schema): (bool, Option<&serde_json::Value>),
    ) -> Result<Request, ProviderError> {
        match self {
            Self::Anthropic(adapter) => adapter
                .build_request(
                    conversation,
                    &system.system_prompt,
                    &system.tools,
                    crate::provider::anthropic::RequestOptions {
                        effort: Some(effort),
                        previous_last_item: previous,
                        disable_tools,
                        output_schema,
                    },
                )
                .map(Request::Anthropic)
                .map_err(|e| e.classification()),
            Self::Chat(adapter) => adapter
                .build_request(
                    conversation,
                    &system.system_prompt,
                    &system.tools,
                    crate::provider::openai_chat::RequestOptions {
                        effort: Some(effort),
                        previous_last_item: previous,
                        disable_tools,
                        output_schema,
                    },
                )
                .map(Request::Chat)
                .map_err(|e| e.classification()),
            Self::Responses(adapter) => adapter
                .build_request(
                    conversation,
                    &system.system_prompt,
                    &system.tools,
                    crate::provider::openai_responses::RequestOptions {
                        effort: Some(effort),
                        previous_last_item: previous,
                        disable_tools,
                        prompt_cache_key: Some(session_id),
                        output_schema,
                    },
                )
                .map(Request::Responses)
                .map_err(|e| e.classification()),
        }
    }

    pub(super) fn stream(
        &self,
        client: &HttpClient,
        request: &Request,
    ) -> Result<Stream, ProviderError> {
        match (self, request) {
            (Self::Anthropic(adapter), Request::Anthropic(request)) => adapter
                .stream(client, request)
                .map(Stream::Anthropic)
                .map_err(|e| e.classification()),
            (Self::Chat(adapter), Request::Chat(request)) => adapter
                .stream(client, request)
                .map(Stream::Chat)
                .map_err(|e| e.classification()),
            (Self::Responses(adapter), Request::Responses(request)) => adapter
                .stream(client, request)
                .map(Stream::Responses)
                .map_err(|e| e.classification()),
            (Self::Anthropic(_), Request::Chat(_) | Request::Responses(_))
            | (Self::Chat(_), Request::Anthropic(_) | Request::Responses(_))
            | (Self::Responses(_), Request::Anthropic(_) | Request::Chat(_)) => {
                Err(ProviderError::NonRetryable {
                    reason: NonRetryableKind::InvalidRequest,
                })
            }
        }
    }
}

impl Stream {
    pub(super) fn cancel(&self) {
        match self {
            Self::Anthropic(stream) => stream.cancellation().cancel(),
            Self::Chat(stream) => stream.cancellation().cancel(),
            Self::Responses(stream) => stream.cancellation().cancel(),
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        match self {
            Self::Anthropic(stream) => stream.is_finished(),
            Self::Chat(stream) => stream.is_finished(),
            Self::Responses(stream) => stream.is_finished(),
        }
    }

    pub(super) fn join(&mut self) -> Result<(), HttpError> {
        match self {
            Self::Anthropic(stream) => stream.join(),
            Self::Chat(stream) => stream.join(),
            Self::Responses(stream) => stream.join(),
        }
    }

    pub(super) fn poll(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, ProviderError> {
        match self {
            Self::Anthropic(stream) => stream.poll(wait).map_err(|e| e.classification()),
            Self::Chat(stream) => stream.poll(wait).map_err(|e| e.classification()),
            Self::Responses(stream) => stream.poll(wait).map_err(|e| e.classification()),
        }
    }

    pub(super) fn finish(self) -> Result<AssistantTurn, ProviderError> {
        match self {
            Self::Anthropic(stream) => stream.finish().map_err(|e| e.classification()),
            Self::Chat(stream) => stream.finish().map_err(|e| e.classification()),
            Self::Responses(stream) => stream.finish().map_err(|e| e.classification()),
        }
    }
}

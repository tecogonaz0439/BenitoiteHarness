//! アダプタの要求とストリームの違いをループから隠す（設計書 02-01「層の分け方」）。

use std::time::Duration;

use crate::context::SystemContext;
use crate::http::{HttpClient, HttpError};
use crate::provider::anthropic::{AnthropicAdapter, AnthropicRequest, AnthropicStream};
use crate::provider::openai_chat::{OpenAiChatAdapter, OpenAiChatRequest, OpenAiChatStream};
use crate::provider::{
    AdapterKind, AssistantTurn, Conversation, NonRetryableKind, ProviderError, StreamEvent,
};

use super::AgentConfig;

pub(super) enum Adapter {
    Anthropic(AnthropicAdapter),
    Chat(OpenAiChatAdapter),
}

pub(super) enum Request {
    Anthropic(AnthropicRequest),
    Chat(OpenAiChatRequest),
}

pub(super) enum Stream {
    Anthropic(AnthropicStream),
    Chat(OpenAiChatStream),
}

fn invalid() -> ProviderError {
    ProviderError::NonRetryable {
        reason: NonRetryableKind::InvalidRequest,
    }
}

impl Adapter {
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
            AdapterKind::OpenAiResponses => return Err(invalid()),
        })
    }

    pub(super) fn request(
        &self,
        conversation: &Conversation,
        system: &SystemContext,
        effort: &str,
        previous: Option<usize>,
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
                        disable_tools: false,
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
                        disable_tools: false,
                    },
                )
                .map(Request::Chat)
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
            (Self::Anthropic(_), Request::Chat(_)) | (Self::Chat(_), Request::Anthropic(_)) => {
                Err(invalid())
            }
        }
    }
}

impl Stream {
    pub(super) fn cancel(&self) {
        match self {
            Self::Anthropic(stream) => stream.cancellation().cancel(),
            Self::Chat(stream) => stream.cancellation().cancel(),
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        match self {
            Self::Anthropic(stream) => stream.is_finished(),
            Self::Chat(stream) => stream.is_finished(),
        }
    }

    pub(super) fn join(&mut self) -> Result<(), HttpError> {
        match self {
            Self::Anthropic(stream) => stream.join(),
            Self::Chat(stream) => stream.join(),
        }
    }

    pub(super) fn poll(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, ProviderError> {
        match self {
            Self::Anthropic(stream) => stream.poll(wait).map_err(|e| e.classification()),
            Self::Chat(stream) => stream.poll(wait).map_err(|e| e.classification()),
        }
    }

    pub(super) fn finish(self) -> Result<AssistantTurn, ProviderError> {
        match self {
            Self::Anthropic(stream) => stream.finish().map_err(|e| e.classification()),
            Self::Chat(stream) => stream.finish().map_err(|e| e.classification()),
        }
    }
}

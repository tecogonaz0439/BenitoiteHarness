//! 道具の順序と引数の契約を固定する（設計書 01-03「道具の定義の共通の規則」）。

use super::{ToolDefinition, ToolError, ToolId, text};
use crate::provider::{ArgumentSpec, ArgumentType, SchemaKind, ToolSpec};
use serde_json::Value;

/// 配列の文字列化と OpenAI strict の null を修復し、既知の欄と型を検証する。
pub fn repair_and_validate(spec: &ToolSpec, arguments: &mut Value) -> Result<(), ToolError> {
    spec.normalize_openai_arguments(arguments);
    let invalid = |detail: &str| {
        ToolError::InvalidArguments(text::invalid_arguments(
            detail,
            &spec.input_schema(SchemaKind::Plain),
        ))
    };
    let object = arguments
        .as_object_mut()
        .ok_or_else(|| invalid(text::OBJECT_REQUIRED))?;
    for name in object.keys() {
        if !spec.arguments.iter().any(|argument| argument.name == *name) {
            return Err(invalid(&text::unknown_argument(name)));
        }
    }
    for argument in &spec.arguments {
        let Some(value) = object.get_mut(&argument.name) else {
            if argument.required {
                return Err(invalid(&text::missing_argument(&argument.name)));
            }
            continue;
        };
        if argument.kind == ArgumentType::StringArray
            && let Some(encoded) = value.as_str()
        {
            *value = serde_json::from_str(encoded)
                .map_err(|_| invalid(&text::wrong_type(&argument.name, argument.kind)))?;
        }
        let valid = match argument.kind {
            ArgumentType::String => value.is_string(),
            ArgumentType::Integer => value.is_i64() || value.is_u64(),
            ArgumentType::Boolean => value.is_boolean(),
            ArgumentType::StringArray => value
                .as_array()
                .is_some_and(|values| values.iter().all(Value::is_string)),
        };
        if !valid {
            return Err(invalid(&text::wrong_type(&argument.name, argument.kind)));
        }
    }
    Ok(())
}

pub(super) fn definitions(mode: crate::config::Mode) -> Vec<ToolDefinition> {
    use ArgumentType::{Boolean, Integer, String};
    let mut tools = vec![
        definition(
            ToolId::Read,
            text::READ,
            &[text::READ.1],
            vec![
                arg("path", String, true),
                arg("offset", Integer, false),
                arg("limit", Integer, false),
            ],
            true,
        ),
        definition(
            ToolId::Glob,
            text::GLOB,
            &[text::GLOB.1],
            vec![arg("pattern", String, true), arg("path", String, false)],
            true,
        ),
        definition(
            ToolId::Grep,
            text::GREP,
            &[text::GREP.1],
            vec![
                arg("pattern", String, true),
                arg("path", String, false),
                arg("glob", String, false),
                arg("ignore_case", Boolean, false),
                arg("context", Integer, false),
                arg("files_only", Boolean, false),
            ],
            true,
        ),
        definition(
            ToolId::Write,
            text::WRITE,
            &[text::WRITE.1],
            vec![arg("path", String, true), arg("content", String, true)],
            false,
        ),
        definition(
            ToolId::Edit,
            text::EDIT,
            &[text::EDIT.1],
            vec![
                arg("path", String, true),
                arg("old_string", String, true),
                arg("new_string", String, true),
                arg("replace_all", Boolean, false),
            ],
            false,
        ),
    ];
    if mode == crate::config::Mode::Normal {
        tools.push(definition(
            ToolId::Bash,
            text::BASH,
            &[text::BASH.1],
            vec![
                arg("command", String, true),
                arg("timeout", Integer, false),
                arg("escalate", Boolean, false),
                arg("justification", String, false),
            ],
            false,
        ));
    }
    tools.push(definition(
        ToolId::WebFetch,
        text::WEB_FETCH,
        &[text::WEB_FETCH.1],
        vec![arg("url", String, true)],
        false,
    ));
    tools
}

fn definition(
    id: ToolId,
    prompt: (&'static str, &'static str),
    rules: &'static [&'static str],
    arguments: Vec<ArgumentSpec>,
    parallel: bool,
) -> ToolDefinition {
    let (summary, _) = prompt;
    ToolDefinition {
        id,
        spec: ToolSpec {
            name: id.name().to_owned(),
            description: summary.to_owned(),
            arguments,
        },
        prompt_key: id.name(),
        summary,
        rules,
        parallel,
    }
}

fn arg(name: &str, kind: ArgumentType, required: bool) -> ArgumentSpec {
    ArgumentSpec {
        name: name.to_owned(),
        kind,
        required,
        description: text::argument(name).to_owned(),
    }
}

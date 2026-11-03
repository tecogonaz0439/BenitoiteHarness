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
    validate_object(&spec.arguments, object, None).map_err(|detail| invalid(&detail))
}

/// 欄の並びでオブジェクトを検証する。`parent` は要素の検証のときの配列の引数の名前と添字で、
/// 誤りの文に入れて、どの要素を直せばよいかを示す。
fn validate_object(
    arguments: &[ArgumentSpec],
    object: &mut serde_json::Map<std::string::String, Value>,
    parent: Option<(&str, usize)>,
) -> Result<(), std::string::String> {
    let qualified = |name: &str| match parent {
        Some((array, index)) => format!("{array}[{index}].{name}"),
        None => name.to_owned(),
    };
    for name in object.keys() {
        if !arguments.iter().any(|argument| argument.name == *name) {
            return Err(text::unknown_argument(&qualified(name)));
        }
    }
    for argument in arguments {
        let name = qualified(&argument.name);
        let Some(value) = object.get_mut(&argument.name) else {
            if argument.required {
                return Err(text::missing_argument(&name));
            }
            continue;
        };
        // 配列を JSON の文字列に包んで送る LLM があるので、文字列なら一度だけ読み直す。
        if matches!(
            argument.kind,
            ArgumentType::StringArray | ArgumentType::ObjectArray
        ) && let Some(encoded) = value.as_str()
        {
            *value = serde_json::from_str(encoded)
                .map_err(|_| text::wrong_type(&name, argument.kind))?;
        }
        let valid = match argument.kind {
            ArgumentType::String => value.is_string(),
            ArgumentType::Integer => value.is_i64() || value.is_u64(),
            ArgumentType::Boolean => value.is_boolean(),
            ArgumentType::StringArray => value
                .as_array()
                .is_some_and(|values| values.iter().all(Value::is_string)),
            ArgumentType::ObjectArray => value
                .as_array()
                .is_some_and(|values| values.iter().all(Value::is_object)),
        };
        if !valid {
            return Err(text::wrong_type(&name, argument.kind));
        }
        if argument.kind == ArgumentType::ObjectArray
            && let Some(values) = value.as_array_mut()
        {
            for (index, item) in values.iter_mut().enumerate() {
                if let Some(item) = item.as_object_mut() {
                    validate_object(&argument.items, item, Some((&argument.name, index)))?;
                }
            }
        }
    }
    Ok(())
}

/// 評価用の複数の置き換えの形の `edit` の引数（設計書 01-03「複数の置き換え」）。
pub(super) fn multiple_edit_arguments() -> Vec<ArgumentSpec> {
    use ArgumentType::{Boolean, ObjectArray, String};
    let mut edits = arg("edits", ObjectArray, true);
    edits.items = vec![
        arg("old_string", String, true),
        arg("new_string", String, true),
        arg("replace_all", Boolean, false),
    ];
    vec![arg("path", String, true), edits]
}

/// 評価用の計画を書く道具（設計書 01-03「update_plan」）。状態を持たず、並べて実行しない。
pub(super) fn plan_definition() -> ToolDefinition {
    use ArgumentType::{ObjectArray, String};
    let mut plan = arg("plan", ObjectArray, true);
    plan.items = vec![arg("step", String, true), arg("status", String, true)];
    definition(
        ToolId::UpdatePlan,
        text::UPDATE_PLAN,
        &[text::UPDATE_PLAN.1],
        vec![arg("explanation", String, false), plan],
        false,
    )
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
        ToolId::BenitoiteCheck,
        text::BENITOITE_CHECK,
        &[text::BENITOITE_CHECK.1],
        vec![
            arg("path", String, true),
            arg_described("tests", Boolean, false, text::CHECK_TESTS),
        ],
        true,
    ));
    if mode == crate::config::Mode::Restricted {
        tools.push(definition(
            ToolId::BenitoiteRun,
            text::BENITOITE_RUN,
            &[text::BENITOITE_RUN.1],
            vec![
                arg("path", String, true),
                arg_described("args", ArgumentType::StringArray, false, text::RUN_ARGS),
                arg_described("stdin", String, false, text::RUN_STDIN),
                arg("timeout", Integer, false),
                arg_described("write_agents", Boolean, false, text::RUN_WRITE_AGENTS),
            ],
            false,
        ));
    }
    if mode == crate::config::Mode::Restricted {
        tools.push(definition(
            ToolId::BenitoiteTest,
            text::BENITOITE_TEST,
            &[text::BENITOITE_TEST.1],
            vec![
                arg("paths", ArgumentType::StringArray, true),
                arg_described("write_agents", Boolean, false, text::RUN_WRITE_AGENTS),
            ],
            false,
        ));
    }
    if mode == crate::config::Mode::Restricted {
        tools.push(definition(
            ToolId::BenitoiteScratch,
            text::BENITOITE_SCRATCH,
            &[text::BENITOITE_SCRATCH.1],
            vec![
                arg("code", String, true),
                arg_described("args", ArgumentType::StringArray, false, text::RUN_ARGS),
                arg_described("stdin", String, false, text::RUN_STDIN),
                arg_described("write_agents", Boolean, false, text::RUN_WRITE_AGENTS),
            ],
            false,
        ));
    }
    tools.push(definition(
        ToolId::BenitoiteExplain,
        text::BENITOITE_EXPLAIN,
        &[text::BENITOITE_EXPLAIN.1],
        vec![arg_described("code", String, true, text::SKILL_CODE)],
        true,
    ));
    tools.push(definition(
        ToolId::BenitoiteReference,
        text::BENITOITE_REFERENCE,
        if mode == crate::config::Mode::Normal {
            &[
                text::BENITOITE_REFERENCE.1,
                text::BENITOITE_REFERENCE_NORMAL,
            ]
        } else {
            &[text::BENITOITE_REFERENCE.1]
        },
        vec![
            arg_described("name", String, false, text::SKILL_NAME),
            arg("offset", Integer, false),
            arg("limit", Integer, false),
        ],
        true,
    ));
    if mode == crate::config::Mode::Restricted {
        tools.push(definition(
            ToolId::CreateProject,
            text::CREATE_PROJECT,
            &[text::CREATE_PROJECT.1],
            vec![arg("path", String, true)],
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
    arg_described(name, kind, required, text::argument(name))
}

fn arg_described(
    name: &str,
    kind: ArgumentType,
    required: bool,
    description: &str,
) -> ArgumentSpec {
    ArgumentSpec {
        name: name.to_owned(),
        kind,
        required,
        description: description.to_owned(),
        items: Vec::new(),
    }
}

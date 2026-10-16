//! 道具の引数の共通の型から JSON Schema を作る（設計書 02-01「道具の定義の変換」）。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// アダプタが提供者ごとの外側の形で包む道具の定義。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// 道具の名前。
    pub name: String,
    /// 道具の説明。
    pub description: String,
    /// 引数の宣言順の並び。
    pub arguments: Vec<ArgumentSpec>,
}

/// 一つの引数の宣言。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArgumentSpec {
    /// 引数の名前。
    pub name: String,
    /// 引数の型。
    pub kind: ArgumentType,
    /// 省略できないか。
    pub required: bool,
    /// 引数の説明。
    pub description: String,
}

/// 試作の道具が使う引数の型。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentType {
    /// 文字列。
    String,
    /// 整数。
    Integer,
    /// 真偽。
    Boolean,
    /// 文字列の配列。
    StringArray,
}

/// 提供者ごとの JSON Schema の形。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaKind {
    /// 必須の引数だけを required に入れる。
    Plain,
    /// 普通の形に additionalProperties: false を付ける。
    AnthropicStrict,
    /// 全引数を必須にし、省略できるものの型に null を加える。
    OpenAiStrict,
}

impl ToolSpec {
    /// 引数のオブジェクトの Schema を作る。strict の印や道具の外側の形は含めない。
    pub fn input_schema(&self, kind: SchemaKind) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for argument in &self.arguments {
            let type_name = match argument.kind {
                ArgumentType::String => "string",
                ArgumentType::Integer => "integer",
                ArgumentType::Boolean => "boolean",
                ArgumentType::StringArray => "array",
            };
            let property_type = if kind == SchemaKind::OpenAiStrict && !argument.required {
                json!([type_name, "null"])
            } else {
                json!(type_name)
            };
            let mut property = Map::new();
            property.insert("type".into(), property_type);
            property.insert("description".into(), json!(argument.description));
            if argument.kind == ArgumentType::StringArray {
                property.insert("items".into(), json!({"type": "string"}));
            }
            properties.insert(argument.name.clone(), Value::Object(property));
            if argument.required || kind == SchemaKind::OpenAiStrict {
                required.push(argument.name.clone());
            }
        }
        let mut schema = Map::new();
        schema.insert("type".into(), json!("object"));
        schema.insert("properties".into(), Value::Object(properties));
        schema.insert("required".into(), json!(required));
        if kind != SchemaKind::Plain {
            schema.insert("additionalProperties".into(), json!(false));
        }
        Value::Object(schema)
    }

    /// OpenAI strict の省略可能な引数の null を取り除く。検証の前に呼ぶ。
    ///
    /// 必須の引数と未知の引数の null、オブジェクトでない値は検証で拒むため残す。
    pub fn normalize_openai_arguments(&self, arguments: &mut Value) {
        let Some(object) = arguments.as_object_mut() else {
            return;
        };
        for argument in self.arguments.iter().filter(|argument| !argument.required) {
            if object.get(&argument.name).is_some_and(Value::is_null) {
                object.remove(&argument.name);
            }
        }
    }
}

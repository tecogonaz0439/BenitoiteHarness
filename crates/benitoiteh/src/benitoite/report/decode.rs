//! 検査の報告を必要な型だけに直接読み、使わない値は構築しない（設計書 04-01）。

use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;
use std::fmt;

use super::{ApprovalSource, ContentHash, Diagnostic, Effect, text};

// 状態ごとに使わない欄が異なる。型の違う値も消費してから不在として扱うことで、
// ok の diagnostics や、組み込みソースのハッシュに新しい受け入れ条件を加えない。
#[derive(Default)]
/// 受け入れの判定に使う文字列・整数・真偽値だけを保持する。
pub(super) enum Scalar {
    #[default]
    Other,
    String(String),
    Unsigned(u64),
    Bool(bool),
}
impl Scalar {
    /// 文字列であった値を返す。
    pub(super) fn string(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
    /// 符号なし整数であった値を返す。
    pub(super) fn unsigned(&self) -> Option<u64> {
        if let Self::Unsigned(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    /// 真偽値であった値を返す。
    pub(super) fn boolean(&self) -> Option<bool> {
        if let Self::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
}

fn skip_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}
fn skip_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}
macro_rules! ignore_scalars {
    () => {
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
    };
}
impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ScalarVisitor;
        impl<'de> Visitor<'de> for ScalarVisitor {
            type Value = Scalar;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(text::REPORT_JSON)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar::Other)
            }
            fn visit_bool<E: serde::de::Error>(self, b: bool) -> Result<Scalar, E> {
                Ok(Scalar::Bool(b))
            }
            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<Scalar, E> {
                Ok(Scalar::Unsigned(n))
            }
            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<Scalar, E> {
                Ok(u64::try_from(n).map_or(Scalar::Other, Scalar::Unsigned))
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Scalar, E> {
                Ok(Scalar::Other)
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Scalar, E> {
                Ok(Scalar::String(s.into()))
            }
            fn visit_string<E: serde::de::Error>(self, s: String) -> Result<Scalar, E> {
                Ok(Scalar::String(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Scalar, A::Error> {
                skip_seq(a)?;
                Ok(Scalar::Other)
            }
            fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Scalar, A::Error> {
                skip_map(a)?;
                Ok(Scalar::Other)
            }
        }
        d.deserialize_any(ScalarVisitor)
    }
}

/// 名前の分かった欄だけを読み取るオブジェクト。
pub(super) trait Fields: Default {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error>;
}
/// オブジェクト以外の値は保持せず、型が違った印だけを残す。
pub(super) struct Object<T>(pub Option<T>);
impl<T> Default for Object<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<'de, T: Fields> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ObjectVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Fields> Visitor<'de> for ObjectVisitor<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(text::REPORT_JSON)
            }
            ignore_scalars!();
            fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
                skip_seq(a)?;
                Ok(Object(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut value = T::default();
                while let Some(key) = map.next_key::<String>()? {
                    value.field(&key, &mut map)?;
                }
                Ok(Object(Some(value)))
            }
        }
        d.deserialize_any(ObjectVisitor(std::marker::PhantomData))
    }
}

/// 要素の構造体を配列全体に保持せず、要る情報だけを集計する。
pub(super) trait Fold: Default {
    type Item: for<'de> Deserialize<'de>;
    fn push(&mut self, item: Self::Item);
}
/// 配列を逐次読み、要素数に比例する中間の木を作らない。
pub(super) struct Array<T>(pub Option<T>);
impl<T> Default for Array<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<'de, T: Fold> Deserialize<'de> for Array<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ArrayVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Fold> Visitor<'de> for ArrayVisitor<T> {
            type Value = Array<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(text::REPORT_JSON)
            }
            ignore_scalars!();
            fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
                skip_map(a)?;
                Ok(Array(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut value = T::default();
                while let Some(item) = seq.next_element::<T::Item>()? {
                    value.push(item);
                }
                Ok(Array(Some(value)))
            }
        }
        d.deserialize_any(ArrayVisitor(std::marker::PhantomData))
    }
}

// Value と同じく、重なった最上位の欄は最後の値を採る。使わない欄は木を作らない。
macro_rules! fields {
    ($name:ident { $($field:ident : $ty:ty => $key:literal),* $(,)? }) => {
        /// 報告のうち名前と型が分かった欄だけを保持する。
        #[derive(Default)]
        pub(super) struct $name { $(pub $field:$ty),* }
        impl Fields for $name {
            fn field<'de,A:MapAccess<'de>>(&mut self,key:&str,map:&mut A)->Result<(),A::Error>{
                match key { $($key=>self.$field=map.next_value()?,)* _=>{let _ignored=map.next_value::<IgnoredAny>()?;} } Ok(())
            }
        }
    }
}
fields!(Header {
    schema:Scalar => "schemaVersion",
    kind:Scalar => "kind",
    status:Scalar => "status",
});
fields!(Success {
    complete:Scalar => "complete",
    executed:Scalar => "executed",
    aggregate:Object<Aggregate>  => "aggregateEffects",
    targets:Array<Targets>  => "targets",
    sources:Array<Sources>  => "sources",
});
fields!(Aggregate {
    basis:Scalar => "basis",
    expanded:Array<Effects>  => "expanded",
});
fields!(Target {
    main:Object<Contract>  => "main",
    shell:Scalar => "processShellReference",
});
fields!(Contract {
    shell:Scalar => "processShellReference",
});
fields!(Source {
    kind:Scalar => "kind",
    path:Scalar => "displayPath",
    hash:Object<Hash>  => "contentHash",
});
fields!(Hash {
    algorithm:Scalar => "algorithm",
    value:Scalar => "value",
});
fields!(Check {
    diagnostics:Option<Vec<Diagnostic>>  => "diagnostics",
});
fields!(Usage {
    diagnostics:Array<Messages>  => "diagnostics",
});
fields!(Message {
    message:Scalar => "message",
});

#[derive(Default)]
/// 展開したエフェクトの集合と、知らない名前・型があった印。
pub(super) struct Effects {
    pub values: BTreeSet<Effect>,
    pub invalid: bool,
}
impl Fold for Effects {
    type Item = Scalar;
    fn push(&mut self, value: Scalar) {
        if let Some(effect) = value.string().and_then(Effect::from_name) {
            self.values.insert(effect);
        } else {
            self.invalid = true;
        }
    }
}
#[derive(Default)]
/// run と test のそれぞれの位置で見つかったシェル参照。
pub(super) struct Targets {
    pub run: bool,
    pub test: bool,
}
impl Fold for Targets {
    type Item = Object<Target>;
    fn push(&mut self, value: Self::Item) {
        if let Some(value) = value.0 {
            self.test |= value.shell.string() == Some("present");
            self.run |= value
                .main
                .0
                .is_some_and(|main| main.shell.string() == Some("present"));
        }
    }
}
#[derive(Default)]
/// 確認に使う利用者のソースと、不正な欄があった印。
pub(super) struct Sources {
    pub values: Vec<ApprovalSource>,
    pub invalid: bool,
}
impl Fold for Sources {
    type Item = Object<Source>;
    fn push(&mut self, value: Self::Item) {
        let Some(source) = value.0 else {
            return;
        };
        if source.kind.string() != Some("user") {
            return;
        }
        if let Some((path, (algorithm, value))) = source.path.string().zip(
            source
                .hash
                .0
                .as_ref()
                .and_then(|h| h.algorithm.string().zip(h.value.string())),
        ) {
            self.values.push(ApprovalSource {
                display_path: path.into(),
                content_hash: ContentHash {
                    algorithm: algorithm.into(),
                    value: value.into(),
                },
            });
        } else {
            self.invalid = true;
        }
    }
}
#[derive(Default)]
/// 使い方の誤りで表示する、最初の診断の文だけを保持する。
pub(super) struct Messages {
    seen: bool,
    pub first: Option<String>,
}
impl Fold for Messages {
    type Item = Object<Message>;
    fn push(&mut self, value: Self::Item) {
        if !self.seen {
            self.seen = true;
            self.first = value.0.and_then(|v| v.message.string().map(str::to_owned));
        }
    }
}

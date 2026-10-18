//! パスをパラメータで渡す Seatbelt の方針を作る（設計書 03-02「サンドボックスの掛け方」「方針の要素」）。

use std::ffi::OsString;

use super::{ProgramAccess, ReadAccess, SandboxPolicy};

/// Seatbelt の方針の文と `sandbox-exec -D` に渡す値。
#[derive(Debug)]
pub struct SeatbeltProfile {
    /// パスの文字列を含まない方針の文。
    pub source: String,
    /// 名前と値の対。方針の文とは別の引数で渡す。
    pub parameters: Vec<(String, OsString)>,
}

impl SeatbeltProfile {
    fn parameter(&mut self, name: String, value: impl Into<OsString>) -> String {
        self.parameters.push((name.clone(), value.into()));
        format!("(param \"{name}\")")
    }
}

fn protected_name_pattern(name: &str) -> String {
    // 検証済みの ASCII の名前だけを受け取り、正規表現の文字を値から持ち込ませない。
    // 大文字小文字を区別しない FS の別名も保護する（実装プラン P1-07「守る場所の判定」）。
    let mut pattern = String::new();
    for character in name.chars() {
        if character.is_ascii_alphabetic() {
            pattern.push('[');
            pattern.push(character.to_ascii_lowercase());
            pattern.push(character.to_ascii_uppercase());
            pattern.push(']');
        } else if character == '.' {
            pattern.push_str("[.]");
        } else {
            pattern.push(character);
        }
    }
    pattern
}

impl SandboxPolicy {
    /// 検証した方針から Seatbelt の文とパラメータを作る。
    pub fn seatbelt_profile(&self) -> SeatbeltProfile {
        let mut profile = SeatbeltProfile {
            source: String::from(
                "(version 1)\n(deny default)\n\
                 ; 子孫の起動と、同じプロセスグループ内へのシグナルに限る。\n\
                 (allow process-fork)\n\
                 ; Rust の標準のライブラリが起動時にページの大きさを読むために必要。\n\
                 ; 拒否すると guard page の確保に失敗して SIGABRT になる。\n\
                 ; 2026-10-17 に、オーケストレータがこの sysctl だけで起動できることを確認した。\n\
                 (allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))\n\
                 (allow signal (target pgrp))\n\
                 ; mach-lookup と、それ以外の sysctl-read は必要性を実測できるまで許さない。\n",
            ),
            parameters: Vec::new(),
        };
        // 出力の破棄はどの方針でも許すが、通常のファイルへの書き込みは広げない
        // （設計書 03-02「方針の要素」）。
        profile.source.push_str(
            "(allow file-write-data (require-all (path \"/dev/null\") (vnode-type CHARACTER-DEVICE)))\n",
        );
        let config = self.config();
        match &config.readable {
            ReadAccess::All => profile
                .source
                .push_str("(allow file-read* file-map-executable)\n"),
            ReadAccess::Roots(paths) => {
                for (i, path) in paths.iter().enumerate() {
                    let param = profile.parameter(format!("READ_{i}"), path.as_os_str());
                    profile.source.push_str(&format!(
                        "(allow file-read* file-map-executable (subpath {param}))\n"
                    ));
                }
            }
        }
        for (i, path) in config.readable_data_roots.iter().enumerate() {
            let param = profile.parameter(format!("READ_DATA_{i}"), path.as_os_str());
            profile
                .source
                .push_str(&format!("(allow file-read* (subpath {param}))\n"));
        }
        for (i, path) in config.readable_files.iter().enumerate() {
            let param = profile.parameter(format!("READ_FILE_{i}"), path.as_os_str());
            profile
                .source
                .push_str(&format!("(allow file-read* (literal {param}))\n"));
        }
        for (i, path) in config.readable_metadata.iter().enumerate() {
            let param = profile.parameter(format!("READ_METADATA_{i}"), path.as_os_str());
            profile
                .source
                .push_str(&format!("(allow file-read-metadata (literal {param}))\n"));
        }
        for (i, path) in config.denied_read.iter().enumerate() {
            let param = profile.parameter(format!("DENIED_READ_{i}"), path.as_os_str());
            profile.source.push_str(&format!(
                "(deny file-read* file-map-executable (subpath {param}))\n"
            ));
        }
        match &config.programs {
            ProgramAccess::Unrestricted => profile.source.push_str("(allow process-exec)\n"),
            ProgramAccess::Only(paths) => {
                for (i, path) in paths.iter().enumerate() {
                    let param = profile.parameter(format!("EXEC_{i}"), path.as_os_str());
                    profile.source.push_str(&format!(
                        "(allow process-exec file-map-executable (literal {param}))\n"
                    ));
                }
            }
        }
        for (i, path) in config.writable.iter().enumerate() {
            let param = profile.parameter(format!("WRITE_{i}"), path.as_os_str());
            profile
                .source
                .push_str(&format!("(allow file-write* (subpath {param}))\n"));
            if config.deny_exec_in_writable {
                profile.source.push_str(&format!(
                    "(deny process-exec file-map-executable (subpath {param}))\n"
                ));
            }
            for (j, name) in config.protected_names.iter().enumerate() {
                let protected =
                    profile.parameter(format!("PROTECTED_{i}_{j}"), protected_name_pattern(name));
                // パスは regex-quote で引用し、守る名前は検証済みのパターンを別の値として渡す。
                // パスの引用は OS 付属の com.apple.CommCenter.sb の home-regex を参照した。
                profile.source.push_str(&format!(
                    "(deny file-write* (regex (string-append \"^\" (regex-quote {param}) \"/(.*/)?\" {protected} \"(/|$)\")))\n"
                ));
            }
        }
        if config.allow_ip {
            // Unix の宛先を許す network* は使わない。名前解決の例外は BH-044 の対象である。
            profile.source.push_str(
                "(allow network-outbound (remote ip \"*:*\"))\n\
                 (allow network-inbound (local ip \"*:*\"))\n\
                 (allow network-bind (local ip \"*:*\"))\n",
            );
        }
        profile
    }
}

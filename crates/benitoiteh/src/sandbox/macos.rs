//! パスをパラメータで渡す Seatbelt の方針を作る（設計書 03-02「サンドボックスの掛け方」「方針の要素」）。

use std::ffi::OsString;

#[cfg(target_os = "macos")]
use super::{PolicyConfig, SandboxError, text};
use super::{ProgramAccess, ReadAccess, SandboxPolicy};
#[cfg(target_os = "macos")]
use std::path::Path;

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

/// 秘密の名前（[`crate::tools::exclude::secret_component`] と同じ規則。`.git` は除く）。
/// どの深さでも、最後の要素がこの名前のファイルとディレクトリを読ませる対象から外す。
const SECRET_NAME_PATTERN: &str = "([.]env|[.]env[.][^/]*|[^/]*[.](pem|key|p12|pfx)|id_(rsa|ecdsa|ed25519)[^/]*|[.]ssh|[.]gnupg|[.]aws|[.]netrc|[.]git-credentials)";
/// 秘密の名前の例外。例と見本の `.env` と、処理系が読む Benitoite のソース（設計書 03-02「制限モードの方針」）。
const SECRET_EXCEPTION_PATTERN: &str = "([.]env[.](example|sample)|[^/]*[.]bnt)";

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

/// 書ける場所の中で、守る名前（根の直下と 3 段までの深さ）がシンボリックリンクなら起動しない。
/// Seatbelt の守りは、リンクを解いた後のパスに名前を当てるので、リンクの先（例: `.agents -> tools/agents`）
/// を守らない。Linux と同じく閉じる側に倒す（設計書 03-02「Linux のファイルシステムの見え方」「.agents の守り」）。
#[cfg(target_os = "macos")]
pub(super) fn reject_protected_links(config: &PolicyConfig) -> Result<(), SandboxError> {
    for root in &config.writable {
        scan_protected_links(config, root, 1)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn scan_protected_links(config: &PolicyConfig, dir: &Path, depth: u8) -> Result<(), SandboxError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SandboxError::Io(error)),
    };
    for entry in entries {
        let entry = entry.map_err(SandboxError::Io)?;
        // lstat の結果で辿るので、リンクの先の木は探さない。
        let file_type = entry.file_type().map_err(SandboxError::Io)?;
        let name = entry.file_name();
        if config
            .protected_names
            .iter()
            .any(|protected| super::same_protected_name(name.as_encoded_bytes(), protected))
        {
            if file_type.is_symlink() {
                return Err(SandboxError::InvalidPolicy(text::WRITABLE_SYMLINK));
            }
        } else if depth < 3 && file_type.is_dir() {
            scan_protected_links(config, &entry.path(), depth.saturating_add(1))?;
        }
    }
    Ok(())
}

impl SandboxPolicy {
    /// 検証した方針から Seatbelt の文とパラメータを作る。
    pub fn seatbelt_profile(&self) -> SeatbeltProfile {
        let mut profile = SeatbeltProfile {
            source: String::from(
                "(version 1)\n(deny default)\n\
                 ; 他のプロセスの引数と環境にある提供者のキーを読ませない（設計書 03-02）。\n\
                 (deny process-info*)\n\
                 (allow process-info* (target self))\n\
                 ; Rust の標準のライブラリが起動時にページの大きさを読むために必要。\n\
                 ; 拒否すると guard page の確保に失敗して SIGABRT になる。\n\
                 ; 2026-10-17 に、オーケストレータがこの sysctl だけで起動できることを確認した。\n\
                 (allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))\n\
                 ; 2026-10-21 に、オーケストレータが sandbox-exec と /usr/bin/sort で実測した。\n\
                 ; 物理メモリの量とページの大きさは秘密を含まず、標準の道具の起動に必要。\n\
                 ; 両方を許したときだけ sort が警告なしで通り、各一名だけでは足りなかった。\n\
                 (allow sysctl-read (sysctl-name \"hw.memsize\"))\n\
                 (allow sysctl-read (sysctl-name \"hw.pagesize\"))\n\
                 (allow signal (target pgrp))\n\
                 ; mach-lookup と、上記三名以外の sysctl-read は必要性を実測できるまで許さない。\n",
            ),
            parameters: Vec::new(),
        };
        // 出力の破棄はどの方針でも許すが、通常のファイルへの書き込みは広げない
        // （設計書 03-02「方針の要素」）。
        profile.source.push_str(
            "(allow file-write-data (require-all (path \"/dev/null\") (vnode-type CHARACTER-DEVICE)))\n",
        );
        let config = self.config();
        if config.allow_fork {
            // 画像の補助には子孫の起動を許さない（設計書 01-03「画像のファイルの読み取り」）。
            profile.source.push_str("(allow process-fork)\n");
        }
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
        for (i, root) in config.secret_name_roots.iter().enumerate() {
            let param = profile.parameter(format!("SECRET_ROOT_{i}"), root.as_os_str());
            // 例外を許す規則を後に置くと、読める範囲の外まで読めるようにしうるので、拒む規則に
            // require-not で例外を組み込む。秘密のディレクトリの下は、例外の名前でも読ませない
            // （`.ssh/x.bnt` など。起動のときに探す仕組みと同じ結果にする）。
            profile.source.push_str(&format!(
                "(deny file-read* file-map-executable (require-all (regex (string-append \"^\" (regex-quote {param}) \"/(.*/)?{SECRET_NAME_PATTERN}$\")) (require-not (regex (string-append \"^\" (regex-quote {param}) \"/(.*/)?{SECRET_EXCEPTION_PATTERN}$\")))))\n\
                 (deny file-read* file-map-executable (regex (string-append \"^\" (regex-quote {param}) \"/(.*/)?{SECRET_NAME_PATTERN}/\")))\n"
            ));
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
        if let Some(port) = config.allow_loopback_port {
            // SBPL は localhost とポートで外向きの接続だけを絞る。
            // bind・inbound・Unix IPC は既定の拒否を保つ（実装プラン P5-34）。
            profile.source.push_str(&format!(
                "(allow network-outbound (remote ip \"localhost:{port}\"))\n"
            ));
        }
        profile
    }
}

//! macOS の Seatbelt の拒否の記録から、一回の起動で拒まれた操作を読む（設計書 03-02「拒否の検出」）。
//!
//! 方針の拒む規則には起動ごとの印を付ける（`with message`）。カーネルの Sandbox の拡張は、印を記録の
//! 末尾に足すので、印で絞れば、ほかの起動の拒否と混ざらない。

/// 一回の起動の拒否の記録を後で読むための手がかり。Linux では何も読まない。
#[derive(Debug)]
pub struct DenialLog {
    #[cfg(target_os = "macos")]
    tag: String,
    #[cfg(target_os = "macos")]
    started: std::time::Instant,
}

/// 一回の起動で拒まれた操作。`shown` は失敗の理由になりやすい順に並べた上限までの数で、
/// 残りは数だけを持つ。
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Denials {
    pub shown: Vec<String>,
    pub omitted: usize,
}

/// 示す拒否の数の上限。多すぎる記録で道具の結果を埋めない。
#[cfg(target_os = "macos")]
const LIMIT: usize = 10;
/// 重なりを除いて覚える拒否の数の上限。
#[cfg(target_os = "macos")]
const COLLECT_LIMIT: usize = 1000;
/// `log show` の出力を読む量の上限。
#[cfg(target_os = "macos")]
const OUTPUT_LIMIT: u64 = 256 * 1024;
/// `log show` を待つ時間の上限。超えたら止め、読めた分だけを使う。
#[cfg(target_os = "macos")]
const WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);

impl DenialLog {
    #[cfg(target_os = "macos")]
    pub(super) fn new(tag: String) -> Self {
        Self {
            tag,
            started: std::time::Instant::now(),
        }
    }

    /// 拒まれた操作と対象を重ねずに返す（例: `file-read-data /Users/a/.ssh/id_ed25519 (cat)`）。
    /// 記録を読めなければ空を返す。`log show` はサンドボックスの外で動く OS の道具である。
    #[cfg(target_os = "macos")]
    pub fn read(&self) -> Denials {
        use std::io::Read;
        use std::process::{Command, Stdio};
        // 起動の少し前からの記録を読む。印で絞るので、範囲を広く取っても混ざらない。
        let seconds = self.started.elapsed().as_secs().saturating_add(2);
        // 送り手をカーネルの Sandbox の拡張に限り、同じ文を書くほかのプロセスの記録を使わない。
        let predicate = format!(
            "processImagePath == \"/kernel\" AND senderImagePath ENDSWITH \"/Sandbox\" AND eventMessage ENDSWITH \"{}\"",
            self.tag
        );
        let mut command = Command::new("/usr/bin/log");
        command
            .args(["show", "--style", "ndjson", "--last"])
            .arg(format!("{seconds}s"))
            .arg("--predicate")
            .arg(predicate)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut child) = super::spawn::spawn(&mut command) else {
            return Denials::default();
        };
        let Some(stdout) = child.stdout.take() else {
            let _killed = child.kill();
            let _waited = child.wait();
            return Denials::default();
        };
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _read = stdout.take(OUTPUT_LIMIT).read_to_end(&mut bytes);
            bytes
        });
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < WAIT_LIMIT => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Ok(None) | Err(_) => {
                    let _killed = child.kill();
                    let _waited = child.wait();
                    break;
                }
            }
        }
        let bytes = reader.join().unwrap_or_default();
        parse(&String::from_utf8_lossy(&bytes), &self.tag)
    }

    /// Linux の Landlock と seccomp は拒否の記録を残さないので、何も返さない。
    #[cfg(not(target_os = "macos"))]
    pub fn read(&self) -> Denials {
        Denials::default()
    }
}

/// `log show --style ndjson` の行から、印の付いた拒否を取り出す。
#[cfg(target_os = "macos")]
fn parse(output: &str, tag: &str) -> Denials {
    let mut denials: Vec<(u8, String)> = Vec::new();
    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(message) = event
            .get("eventMessage")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        // 形は「Sandbox: <名前>(<番号>) deny(<回数>) <操作> <対象>\n<印>」である。
        let Some(body) = message
            .strip_suffix(tag)
            .and_then(|rest| rest.strip_suffix('\n'))
            .and_then(|rest| rest.strip_prefix("Sandbox: "))
        else {
            continue;
        };
        let Some((process, rest)) = body.split_once(") deny(") else {
            continue;
        };
        let Some((_, denied)) = rest.split_once(") ") else {
            continue;
        };
        let name = process.rsplit_once('(').map_or(process, |(name, _)| name);
        // dyld が起動のたびに開こうとする DTrace の補助は、コマンドの失敗の理由にならない。
        if denied == "file-write-data /dev/dtracehelper" {
            continue;
        }
        // 対象の名前は LLM が選びうるので、制御の文字で道具の結果の行を偽らせない。
        let entry: String = format!("{denied} ({name})")
            .chars()
            .map(|character| {
                if character.is_control() {
                    '?'
                } else {
                    character
                }
            })
            .collect();
        // シェルなどは起動のたびに sysctl と mach の名前を引いて拒まれる。コマンドの失敗の理由になりやすい
        // ファイル、通信、プログラムの起動の拒否を先に示す。
        let rank = ["file-", "network-", "process-"]
            .iter()
            .position(|prefix| denied.starts_with(prefix))
            .and_then(|position| u8::try_from(position).ok())
            .unwrap_or(u8::MAX);
        if denials.len() < COLLECT_LIMIT && !denials.iter().any(|(_, known)| *known == entry) {
            denials.push((rank, entry));
        }
    }
    denials.sort_by_key(|(rank, _)| *rank);
    let omitted = denials.len().saturating_sub(LIMIT);
    denials.truncate(LIMIT);
    Denials {
        shown: denials.into_iter().map(|(_, entry)| entry).collect(),
        omitted,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::parse;

    // 作成時の関門: 記録の形の解釈は、実機の拒否を確かめる sandbox_bash の行より細かい分岐を持つ。
    // 印の違う記録、形の崩れた記録、DTrace の補助、重なり、制御の文字、並べ方と上限を確かめる。
    // ほかの起動の記録と制御の文字を含む記録は、実機の起動では決まった形で作れない。
    #[test]
    fn denial_records_are_filtered_by_tag_and_kept_on_one_line() {
        let tag = "benitoiteh-denial-1";
        let event = |message: &str| serde_json::json!({ "eventMessage": message }).to_string();
        let output = [
            event(&format!(
                "Sandbox: cat(11) deny(1) file-write-data /dev/dtracehelper\n{tag}"
            )),
            event(&format!(
                "Sandbox: cat(11) deny(1) file-read-data /home/.ssh/id\n{tag}"
            )),
            event(&format!(
                "Sandbox: cat(11) deny(2) file-read-data /home/.ssh/id\n{tag}"
            )),
            event("Sandbox: cat(11) deny(1) file-read-data /other\nbenitoiteh-denial-2"),
            event(&format!("not a sandbox record\n{tag}")),
            "not json".to_owned(),
            event(&format!(
                "Sandbox: sh(12) deny(1) file-write-create /p/a\nb\n{tag}"
            )),
        ]
        .join("\n");
        let parsed = parse(&output, tag);
        assert_eq!(
            parsed.shown,
            [
                "file-read-data /home/.ssh/id (cat)",
                "file-write-create /p/a?b (sh)",
            ]
        );
        assert_eq!(parsed.omitted, 0);
        // シェルの起動の拒否で上限が埋まっても、ファイルの拒否を先に示し、残りは数だけを示す。
        let mut noisy: Vec<String> = (0..12)
            .map(|i| {
                event(&format!(
                    "Sandbox: sh(12) deny(1) sysctl-read kern.name{i}\n{tag}"
                ))
            })
            .collect();
        noisy.push(event(&format!(
            "Sandbox: sh(12) deny(1) file-write-create /p/out\n{tag}"
        )));
        let parsed = parse(&noisy.join("\n"), tag);
        assert_eq!(
            parsed.shown.first().map(String::as_str),
            Some("file-write-create /p/out (sh)")
        );
        assert_eq!((parsed.shown.len(), parsed.omitted), (10, 3));
    }
}

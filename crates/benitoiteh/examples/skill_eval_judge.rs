//! 通常モードの成果物を既存のサンドボックスで判定する（設計書 07-02「通常モードの課題の作り方」）。
//!
//! JSON の possibly_remaining は、閉じないパイプ、今回の環境の印、書ける場所の下の cwd を検出した印である。
//! false は子孫がいないことの保証ではない。macOS 27 の Apple 製プログラムなどは ps -E に環境が出ない。
//! 印が見えず（削除して exec した場合も含む）、cwd も書ける場所の外へ移した子孫は検出できない。
//! cwd の検査では、同じ場所を使う無関係な同 UID のプロセスも検出することがある。
//! stop_required が true なら後続の評価を止める。
//! success は終了状態と検出結果による判定であり、全子孫の終了を保証しない。

use std::io::{self, Write};
use std::process::ExitCode;

use serde::Serialize;

/// Python の呼び出し元へ、一つの JSON オブジェクトとして返す結果。
#[derive(Serialize)]
struct Report {
    schema_version: u8,
    success: bool,
    exit_code: Option<i32>,
    signal: Option<i32>,
    end: &'static str,
    timed_out: bool,
    /// 残りの兆候を検出した、または実行・後始末の失敗で残りを否定できなかった印。
    /// Apple 製プログラムなどの見えない環境や、印の削除と cwd の移動を併用した子孫は検出できない。
    possibly_remaining: bool,
    /// 検出した残りや後始末の不確実さにより、後続の評価を止める必要がある。
    stop_required: bool,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
    error: Option<String>,
}

impl Report {
    fn failure(error: impl ToString) -> Self {
        Self {
            schema_version: 1,
            success: false,
            exit_code: None,
            signal: None,
            end: "error",
            timed_out: false,
            possibly_remaining: false,
            stop_required: false,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            error: Some(error.to_string()),
        }
    }
}

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    let report = macos::judge();
    #[cfg(not(target_os = "macos"))]
    let report = Report::failure(text::UNSUPPORTED);

    let mut stdout = io::stdout().lock();
    if let Err(error) = serde_json::to_writer(&mut stdout, &report)
        .map_err(io::Error::other)
        .and_then(|()| writeln!(stdout))
    {
        let _written = writeln!(io::stderr(), "{} {error}", text::OUTPUT);
        return ExitCode::FAILURE;
    }
    if report.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(target_os = "macos")]
mod macos {
    //! 方針と環境の組み立てだけを持ち、起動と後始末は sandbox に任せる（設計書 03-02）。

    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fmt;
    use std::fs;
    use std::io::{self, Write};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use benitoiteh::config::paths::DataPaths;
    use benitoiteh::sandbox::run::{Leftovers, RunEnd, RunLimits, run};
    use benitoiteh::sandbox::session::ResourceLimits;
    use benitoiteh::sandbox::{
        PolicyConfig, SandboxCommand, SandboxError, SandboxPolicy, SandboxStdin,
    };

    use super::{Report, text};

    const OUTPUT_LIMIT: usize = 1024 * 1024;
    const RUN_MARKER: &str = "SKILL_EVAL_JUDGE_RUN";

    enum JudgeError {
        Arguments,
        Path,
        Rejected(&'static str),
        Config(benitoiteh::config::ConfigError),
        Io(io::Error),
        Sandbox(SandboxError),
    }

    impl fmt::Display for JudgeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Arguments => f.write_str(text::USAGE),
                Self::Path => f.write_str(text::PATH),
                Self::Rejected(reason) => f.write_str(reason),
                Self::Config(error) => error.fmt(f),
                Self::Io(error) => write!(f, "{} {error}", text::PREPARE),
                Self::Sandbox(error) => error.fmt(f),
            }
        }
    }

    impl From<io::Error> for JudgeError {
        fn from(error: io::Error) -> Self {
            Self::Io(error)
        }
    }

    impl From<SandboxError> for JudgeError {
        fn from(error: SandboxError) -> Self {
            Self::Sandbox(error)
        }
    }

    struct Arguments {
        helper: PathBuf,
        cwd: PathBuf,
        temporary: PathBuf,
        timeout: Duration,
        denied_read: Vec<PathBuf>,
        program: PathBuf,
        args: Vec<OsString>,
    }

    fn real_path(value: OsString) -> Result<PathBuf, JudgeError> {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(JudgeError::Path);
        }
        let canonical = fs::canonicalize(path)?;
        let fd = rustix::fs::open(
            &canonical,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(io::Error::from)?;
        // realpath では残る firmlink の別名を、Seatbelt が照合するカーネルのパスへ直す。
        // F_GETPATH に失敗した入力を、字面だけの方針へ戻さない（レビュー 中-2）。
        let kernel = rustix::fs::getpath(&fd).map_err(io::Error::from)?;
        Ok(PathBuf::from(OsString::from_vec(kernel.into_bytes())))
    }

    fn writable_path(value: OsString) -> Result<PathBuf, JudgeError> {
        let path = Path::new(&value);
        if path.components().any(|part| part == Component::ParentDir) {
            return Err(JudgeError::Rejected(text::WRITABLE_PATH));
        }
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(JudgeError::Rejected(text::WRITABLE_LINK));
        }
        let kernel = real_path(value.clone())?;
        if leading_alias(path).as_os_str() != kernel.as_os_str() {
            return Err(JudgeError::Rejected(text::WRITABLE_PATH));
        }
        Ok(kernel)
    }

    fn leading_alias(path: &Path) -> PathBuf {
        let bytes = path.as_os_str().as_bytes();
        for prefix in ["/tmp", "/var", "/etc"] {
            if bytes == prefix.as_bytes() || bytes.starts_with(format!("{prefix}/").as_bytes()) {
                let mut normalized = b"/private".to_vec();
                normalized.extend_from_slice(bytes);
                return PathBuf::from(OsString::from_vec(normalized));
            }
        }
        path.to_owned()
    }

    impl Arguments {
        fn parse() -> Result<Self, JudgeError> {
            let mut helper = None;
            let mut cwd = None;
            let mut temporary = None;
            let mut timeout = None;
            let mut denied_read = Vec::new();
            let mut args = std::env::args_os().skip(1);
            while let Some(option) = args.next() {
                if option == "--" {
                    let program = real_path(args.next().ok_or(JudgeError::Arguments)?)?;
                    return Ok(Self {
                        helper: helper.ok_or(JudgeError::Arguments)?,
                        cwd: cwd.ok_or(JudgeError::Arguments)?,
                        temporary: temporary.ok_or(JudgeError::Arguments)?,
                        timeout: timeout.ok_or(JudgeError::Arguments)?,
                        denied_read,
                        program,
                        args: args.collect(),
                    });
                }
                let value = args.next().ok_or(JudgeError::Arguments)?;
                match option.to_str() {
                    Some("--helper") if helper.is_none() => helper = Some(real_path(value)?),
                    Some("--cwd") if cwd.is_none() => cwd = Some(writable_path(value)?),
                    Some("--temp") if temporary.is_none() => {
                        temporary = Some(writable_path(value)?)
                    }
                    Some("--deny-read") => denied_read.push(real_path(value)?),
                    Some("--timeout") if timeout.is_none() => {
                        let seconds = value
                            .to_str()
                            .and_then(|value| value.parse::<u64>().ok())
                            .filter(|seconds| *seconds > 0)
                            .ok_or(JudgeError::Arguments)?;
                        timeout = Some(Duration::from_secs(seconds));
                    }
                    _ => return Err(JudgeError::Arguments),
                }
            }
            Err(JudgeError::Arguments)
        }
    }

    // 親の環境をコピーしない。評価の包みの PATH、キー、Python の探索先、
    // 元の workspace の場所を子へ持ち込まない（実装プラン P5-31「作るもの」）。
    fn environment(
        cwd: &Path,
        temporary: &Path,
        marker: &str,
    ) -> Result<BTreeMap<OsString, OsString>, JudgeError> {
        let mut env = BTreeMap::new();
        env.insert(RUN_MARKER.into(), marker.into());
        for (name, value) in [
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
            ("LC_ALL", "C"),
            ("PYTHONNOUSERSITE", "1"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
            ("BENITOITEH_SANDBOX", "workspace-write"),
            ("BENITOITEH_SANDBOX_NETWORK_DISABLED", "1"),
        ] {
            env.insert(name.into(), value.into());
        }
        for (name, child) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            let path = temporary.join(child);
            fs::create_dir(&path)?;
            env.insert(name.into(), path.into_os_string());
        }
        env.insert("PWD".into(), cwd.as_os_str().to_owned());
        for name in ["TMPDIR", "TMP", "TEMP"] {
            env.insert(name.into(), temporary.as_os_str().to_owned());
        }
        Ok(env)
    }

    fn check_descriptors() -> Result<(), JudgeError> {
        // 列挙自身の記述子を閉じてから調べる。起動直後の単一スレッドで行い、
        // 検査の間に別のスレッドが同じ番号を開くことを避ける（レビュー 低-1）。
        let descriptors = fs::read_dir("/dev/fd")?
            .map(|entry| Ok(entry?.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        for name in descriptors {
            let number = name
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
                .ok_or(JudgeError::Rejected(text::DESCRIPTORS))?;
            if number < 3 {
                continue;
            }
            match fs::metadata(Path::new("/dev/fd").join(name)) {
                Ok(_) => return Err(JudgeError::Rejected(text::DESCRIPTORS)),
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        || error.raw_os_error() == Some(rustix::io::Errno::BADF.raw_os_error()) => {
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn prepare_policy(arguments: &Arguments) -> Result<SandboxPolicy, JudgeError> {
        let paths = DataPaths::from_env().map_err(JudgeError::Config)?;
        let home = real_path(paths.home.as_os_str().to_owned())?;
        let executable = real_path(std::env::current_exe()?.into_os_string())?;
        let expected_helper = executable
            .parent()
            .and_then(Path::parent)
            .ok_or(JudgeError::Path)?
            .join("benitoiteh");
        if arguments.helper != real_path(expected_helper.into_os_string())? {
            return Err(JudgeError::Rejected(text::HELPER));
        }
        let mut config =
            PolicyConfig::new(vec![arguments.cwd.clone(), arguments.temporary.clone()]);
        config.allow_ip = false;
        config.deny_exec_in_writable = false;
        config.denied_read = arguments.denied_read.clone();
        // cli::denied_candidates と同じ候補に Keychains を足す。内容は読まない
        // （設計書 03-02「通常モードの方針」、レビュー 中-1）。
        let candidates = [paths.config_dir, paths.data_dir]
            .into_iter()
            .chain([".ssh", ".gnupg", ".aws", "Library/Keychains"].map(|name| home.join(name)));
        for candidate in candidates {
            match fs::symlink_metadata(&candidate) {
                Ok(_) => config
                    .denied_read
                    .push(real_path(candidate.into_os_string())?),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        for writable in &config.writable {
            if home.starts_with(writable)
                || executable.starts_with(writable)
                || arguments.helper.starts_with(writable)
                || writable.parent().is_none()
                || config
                    .denied_read
                    .iter()
                    .any(|denied| writable.starts_with(denied) || denied.starts_with(writable))
            {
                return Err(JudgeError::Rejected(text::WRITABLE));
            }
        }
        if arguments.cwd.starts_with(&arguments.temporary)
            || arguments.temporary.starts_with(&arguments.cwd)
        {
            return Err(JudgeError::Rejected(text::WRITABLE));
        }
        if fs::read_dir(&arguments.temporary)?
            .next()
            .transpose()?
            .is_some()
        {
            return Err(JudgeError::Rejected(text::TEMPORARY));
        }
        Ok(SandboxPolicy::new(config)?)
    }

    fn new_run_marker() -> Result<String, JudgeError> {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    fn marked_processes_remain(marker: &str) -> Result<bool, JudgeError> {
        let output = Command::new("/bin/ps")
            .args(["-axE", "-o", "pid=,command="])
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Err(JudgeError::Rejected(text::PROCESSES));
        }
        Ok(has_run_marker(&output.stdout, marker))
    }

    fn has_run_marker(source: &[u8], marker: &str) -> bool {
        // ps -E の他の環境の値は解かず、今回の印だけを探す。UTF-8 でない値も許す。
        // 無関係な孤児を拾わず、印を消して exec した子孫は検出できないと扱う。
        let entry = format!("{RUN_MARKER}={marker}");
        source
            .windows(entry.len())
            .any(|bytes| bytes == entry.as_bytes())
    }

    fn writable_processes_remain(roots: &[PathBuf]) -> Result<bool, JudgeError> {
        let output = Command::new("/usr/sbin/lsof")
            .args(["-a", "-d", "cwd", "-u"])
            .arg(rustix::process::getuid().as_raw().to_string())
            .arg("-Fpn")
            // 検査の子自身が、実行器から継いだ cwd を残りと誤認させない。
            .current_dir("/")
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Err(JudgeError::Rejected(text::PROCESSES));
        }
        has_writable_cwd(&output.stdout, roots)
    }

    fn has_writable_cwd(source: &[u8], roots: &[PathBuf]) -> Result<bool, JudgeError> {
        let invalid = || JudgeError::Rejected(text::PROCESSES);
        let mut process = false;
        let mut own_process = false;
        let mut seen_cwd = false;
        let mut remaining = false;
        for line in source
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            match line.split_first() {
                Some((b'p', pid)) => {
                    if process && !seen_cwd {
                        return Err(invalid());
                    }
                    let pid = std::str::from_utf8(pid)
                        .ok()
                        .and_then(|value| value.parse::<i64>().ok())
                        .filter(|pid| *pid > 0)
                        .ok_or_else(invalid)?;
                    // 実行器自身は子孫ではない。cwd を引数と同じにした起動も許す。
                    own_process = pid == i64::from(std::process::id());
                    process = true;
                    seen_cwd = false;
                }
                Some((b'n', name)) if process => {
                    let path = PathBuf::from(OsString::from_vec(name.to_vec()));
                    if !path.is_absolute() {
                        return Err(invalid());
                    }
                    let path = leading_alias(&path);
                    remaining |=
                        !own_process && roots.iter().any(|root| path.starts_with(lsof_path(root)));
                    seen_cwd = true;
                }
                Some((b'f', b"cwd")) if process => {}
                _ => return Err(invalid()),
            }
        }
        if !seen_cwd {
            return Err(invalid());
        }
        Ok(remaining)
    }

    fn lsof_path(path: &Path) -> PathBuf {
        // lsof(8) の LC_ALL=C の表示へ根の方を直す。ほかのプロセスの名前を
        // 逆変換すると、字面の「\\n」と改行の表示を取り違えうるためである。
        // 表示の衝突は検出を増やす側に扱い、同 UID のほかの環境は解かない。
        let mut result = Vec::new();
        for byte in path.as_os_str().as_bytes() {
            let escaped = match *byte {
                8 => Some("\\b"),
                12 => Some("\\f"),
                b'\n' => Some("\\n"),
                b'\r' => Some("\\r"),
                b'\t' => Some("\\t"),
                127 => Some("^?"),
                _ => None,
            };
            if let Some(escaped) = escaped {
                result.extend_from_slice(escaped.as_bytes());
            } else if byte.is_ascii_control() {
                result.extend_from_slice(&[b'^', byte.saturating_add(b'@')]);
            } else if byte.is_ascii() {
                result.push(*byte);
            } else {
                result.extend_from_slice(format!("\\x{byte:02x}").as_bytes());
            }
        }
        PathBuf::from(OsString::from_vec(result))
    }

    #[cfg(test)]
    mod tests {
        //! ps -E の印と lsof の cwd の解析を、OS のサンドボックスなしで確かめる（設計書 07-01）。

        use super::{has_run_marker, has_writable_cwd};
        use std::path::PathBuf;

        // 関門: Apple 製の子の環境が見えなくても cwd を拾い、似た名前の外の場所は拾わない。
        #[test]
        fn process_cwd_records_match_only_writable_roots_and_descendants()
        -> Result<(), Box<dyn std::error::Error>> {
            let roots = [
                PathBuf::from("/private/tmp/judge"),
                PathBuf::from("/private/tmp/temp space"),
            ];
            for (source, expected) in [
                ("p100\nfcwd\nn/private/tmp/judge\n", true),
                ("p100\nfcwd\nn/private/tmp/judge/subdir\n", true),
                ("p100\nfcwd\nn/tmp/temp space/child\n", true),
                (
                    "p100\nfcwd\nn/private/tmp/judge-other\np101\nfcwd\nn/\n",
                    false,
                ),
                ("p100\nn/\np101\nn/private/tmp/temp space\n", true),
            ] {
                assert_eq!(
                    has_writable_cwd(source.as_bytes(), &roots)
                        .map_err(|error| error.to_string())?,
                    expected,
                    "{source}"
                );
            }
            let own = format!("p{}\nfcwd\nn/private/tmp/judge\n", std::process::id());
            assert!(!has_writable_cwd(own.as_bytes(), &roots).map_err(|error| error.to_string())?);
            Ok(())
        }

        // 関門: 文字のエスケープで包含関係を逃さず、壊れた一覧を残りなしと扱わない。
        #[test]
        fn process_cwd_records_match_escaped_names_and_reject_incomplete_output()
        -> Result<(), Box<dyn std::error::Error>> {
            let roots = [PathBuf::from("/private/tmp/判定\nroot")];
            assert!(
                has_writable_cwd(
                    b"p101\nfcwd\nn/private/tmp/\\xe5\\x88\\xa4\\xe5\\xae\\x9a\\nroot/subdir\n",
                    &roots
                )
                .map_err(|error| error.to_string())?
            );
            assert!(
                has_writable_cwd(
                    b"p101\nfcwd\nn/private/tmp/literal\\nroot/child\n",
                    &[PathBuf::from("/private/tmp/literal\\nroot")]
                )
                .map_err(|error| error.to_string())?
            );
            for source in [
                "",
                "pbad\nn/\n",
                "p0\nn/\n",
                "n/\n",
                "p10\n",
                "p10\nnrelative\n",
                "p10\np11\nn/\n",
            ] {
                assert!(
                    has_writable_cwd(source.as_bytes(), &roots).is_err(),
                    "{source}"
                );
            }
            Ok(())
        }

        // 関門: 今回の印を持つ残りだけを拾い、無関係なプロセスや別の判定の残りで停止しない。
        #[test]
        fn process_environment_matches_only_the_current_run_marker() {
            let marker = "0123456789abcdef0123456789abcdef";
            for (source, expected) in [
                (
                    "5000 /bin/probe HOME=/tmp/home SKILL_EVAL_JUDGE_RUN=0123456789abcdef0123456789abcdef LANG=C\n",
                    true,
                ),
                (
                    "5001 /bin/build PPID=1 HOME=/tmp/home\n5002 /bin/agent PPID=1\n",
                    false,
                ),
                (
                    "5003 /bin/probe SKILL_EVAL_JUDGE_RUN=ffffffffffffffffffffffffffffffff\n",
                    false,
                ),
                (
                    "5004 /bin/probe OTHER_RUN=0123456789abcdef0123456789abcdef\n",
                    false,
                ),
                ("5005 /bin/probe SKILL_EVAL_JUDGE_RUN=\n", false),
                ("", false),
            ] {
                assert_eq!(
                    has_run_marker(source.as_bytes(), marker),
                    expected,
                    "{source}"
                );
            }
        }

        // 関門: 他の値の文字コード・空白・長さを解こうとして、検索が誤りになったり切れたりしない。
        #[test]
        fn process_environment_search_ignores_other_values_and_non_utf8() {
            let marker = "0123456789abcdef0123456789abcdef";
            let mut source = b"\n\t5000 /bin/probe OTHER=\xff\xfe mixed value \t".to_vec();
            source.extend_from_slice(&[b'x'; 1024]);
            assert!(!has_run_marker(&source, marker));
            source.extend_from_slice(b" SKILL_EVAL_JUDGE_RUN=0123456789abcdef0123456789abcdef\r\n");
            assert!(has_run_marker(&source, marker));
        }
    }

    fn resource_limits(timeout: Duration) -> Result<ResourceLimits, JudgeError> {
        // CPU 時間は全スレッドの合計。壁時計の時間内に全 CPU を使う正しい判定も許す。
        // 実行器には掛けず、回収と JSON のための CPU 時間を確保する（設計書 03-02）。
        let cpus =
            u64::try_from(std::thread::available_parallelism()?.get()).map_err(io::Error::other)?;
        Ok(ResourceLimits {
            cpu_seconds: Some(timeout.as_secs().saturating_mul(cpus).saturating_add(5)),
            file_size_bytes: Some(64 * 1024 * 1024),
        })
    }

    #[derive(Default)]
    struct Capture {
        bytes: Vec<u8>,
        truncated: bool,
    }

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let keep = OUTPUT_LIMIT
                .saturating_sub(self.bytes.len())
                .min(bytes.len());
            self.bytes
                .extend_from_slice(bytes.get(..keep).unwrap_or_default());
            self.truncated |= keep < bytes.len();
            // 保存を打ち切った後も、全バイトを受け取ったと返してパイプを読み続ける。
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn captured(sink: &Mutex<Capture>) -> (String, bool) {
        let capture = sink.lock().unwrap_or_else(PoisonError::into_inner);
        (
            String::from_utf8_lossy(&capture.bytes).into_owned(),
            capture.truncated,
        )
    }

    pub(super) fn judge() -> Report {
        match execute() {
            Ok(report) => report,
            Err(error) => Report::failure(error),
        }
    }

    fn execute() -> Result<Report, JudgeError> {
        check_descriptors()?;
        let arguments = Arguments::parse()?;
        if !arguments.helper.is_file() || !arguments.program.is_file() {
            return Err(JudgeError::Path);
        }
        let policy = prepare_policy(&arguments)?;
        let marker = new_run_marker()?;
        let roots = [arguments.cwd.clone(), arguments.temporary.clone()];
        let command = SandboxCommand {
            policy,
            program: arguments.program,
            args: arguments.args,
            env: environment(&arguments.cwd, &arguments.temporary, &marker)?,
            cwd: arguments.cwd,
            stdin: SandboxStdin::Null,
        }
        .to_command_with_limits(&arguments.helper, resource_limits(arguments.timeout)?)?;
        let stdout = Arc::new(Mutex::new(Capture::default()));
        let stderr = Arc::new(Mutex::new(Capture::default()));
        let result = run(
            command,
            Vec::new(),
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            RunLimits {
                timeout: arguments.timeout,
                ..RunLimits::default()
            },
            &AtomicBool::new(false),
        );
        let mut report = Report::failure(text::RUN);
        (report.stdout, report.stdout_truncated) = captured(&stdout);
        (report.stderr, report.stderr_truncated) = captured(&stderr);
        // 同じ UID の環境が見える ps -E で、回収後に今回の印を持つものだけを探す。
        // 出力はパイプなので列幅の上限はなく、環境の値を診断や記録へ出さない。
        let marked = marked_processes_remain(&marker);
        let writable = writable_processes_remain(&roots);
        let remaining = marked.and_then(|marked| writable.map(|writable| marked || writable));
        match result {
            Ok(outcome) => {
                report.exit_code = outcome.status.code();
                report.signal = outcome.status.signal();
                report.timed_out = outcome.end == RunEnd::TimedOut;
                report.end = match outcome.end {
                    RunEnd::Exited => "exited",
                    RunEnd::TimedOut => "timed_out",
                    RunEnd::Cancelled => "cancelled",
                };
                report.possibly_remaining = outcome.leftovers == Leftovers::PossiblyRemaining
                    || remaining.as_ref().map_or(true, |remaining| *remaining);
                report.stop_required = report.possibly_remaining;
                report.error = if report.possibly_remaining {
                    Some(text::LEFTOVERS.into())
                } else if report.timed_out {
                    Some(text::TIMEOUT.into())
                } else if report.signal == Some(24) {
                    Some(text::CPU_LIMIT.into())
                } else if report.signal == Some(25) {
                    Some(text::FILE_LIMIT.into())
                } else if report.stdout_truncated || report.stderr_truncated {
                    Some(text::TRUNCATED.into())
                } else if !outcome.status.success() || outcome.end != RunEnd::Exited {
                    Some(text::CHILD_FAILED.into())
                } else {
                    None
                };
                report.success = report.error.is_none();
            }
            Err(error) => {
                // 後始末の失敗も含みうるので、次の判定へ進めない。
                report.error = Some(error.to_string());
                report.possibly_remaining = true;
                report.stop_required = true;
            }
        }
        if let Err(error) = remaining {
            report.error = Some(error.to_string());
            report.success = false;
            report.possibly_remaining = true;
            report.stop_required = true;
        }
        Ok(report)
    }
}

mod text {
    //! 実行器の診断の英文（設計書 06-01「lint と実装の規約」）。

    #[cfg(not(target_os = "macos"))]
    pub const UNSUPPORTED: &str =
        "skill_eval_judge requires macOS sandbox-exec; this OS is unsupported.";
    pub const OUTPUT: &str = "Could not write the judge result:";
    #[cfg(target_os = "macos")]
    pub const USAGE: &str = "Usage: skill_eval_judge --helper PATH --cwd DIR --temp DIR --timeout SECONDS [--deny-read PATH]... -- PROGRAM [ARG]...";
    #[cfg(target_os = "macos")]
    pub const PATH: &str =
        "Judge paths must be absolute paths to existing files or directories of the required kind.";
    #[cfg(target_os = "macos")]
    pub const PREPARE: &str = "Could not prepare the judge:";
    #[cfg(target_os = "macos")]
    pub const RUN: &str = "Could not run the judge.";
    #[cfg(target_os = "macos")]
    pub const LEFTOVERS: &str = "Descendant processes may remain; stop subsequent evaluation.";
    #[cfg(target_os = "macos")]
    pub const TRUNCATED: &str = "Judge output exceeded the 1 MiB limit per stream.";
    #[cfg(target_os = "macos")]
    pub const TIMEOUT: &str = "The judge timed out.";
    #[cfg(target_os = "macos")]
    pub const CHILD_FAILED: &str = "The judge command failed.";
    #[cfg(target_os = "macos")]
    pub const CPU_LIMIT: &str = "The judge command exceeded its CPU time limit (SIGXCPU).";
    #[cfg(target_os = "macos")]
    pub const FILE_LIMIT: &str = "The judge command exceeded its file size limit (SIGXFSZ).";
    #[cfg(target_os = "macos")]
    pub const WRITABLE_LINK: &str = "Writable roots must not end in a symbolic link.";
    #[cfg(target_os = "macos")]
    pub const WRITABLE_PATH: &str = "Writable roots must have no '..' components and must match their kernel paths, except leading /tmp, /var, or /etc aliases.";
    #[cfg(target_os = "macos")]
    pub const WRITABLE: &str =
        "Writable roots overlap private paths, executable paths, or each other.";
    #[cfg(target_os = "macos")]
    pub const TEMPORARY: &str = "The judge temporary directory must be empty.";
    #[cfg(target_os = "macos")]
    pub const HELPER: &str =
        "The helper must be the benitoiteh executable beside the examples directory.";
    #[cfg(target_os = "macos")]
    pub const DESCRIPTORS: &str =
        "The judge inherited file descriptors other than standard input, output, and error.";
    #[cfg(target_os = "macos")]
    pub const PROCESSES: &str =
        "Could not inspect processes for detected leftovers; stop subsequent evaluation.";
}

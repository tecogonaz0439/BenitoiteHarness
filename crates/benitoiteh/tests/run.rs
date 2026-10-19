//! 実プロセスで時間の上限・取り消し・回収前のグループの掃除を確かめる（設計書 03-02、実装プラン P2-01）。

#[path = "support/process.rs"]
mod support;

use benitoiteh::sandbox::run::{Leftovers, RunEnd, RunLimits, StopSignal, run};
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use support::{ProcessFixture, TestResult};

fn command(f: &ProcessFixture, args: &[&str]) -> std::process::Command {
    let mut command = benitoiteh::sandbox::session::command(
        std::path::Path::new(env!("CARGO_BIN_EXE_benitoiteh")),
        &f.probe,
    );
    command.args(args).current_dir(&f.root);
    command
}
fn limits() -> RunLimits {
    RunLimits {
        timeout: Duration::from_secs(5),
        grace: Duration::from_millis(100),
        pipe_close_wait: Duration::from_millis(100),
        ..RunLimits::default()
    }
}
fn sink() -> Arc<Mutex<Vec<u8>>> {
    Arc::new(Mutex::new(Vec::new()))
}
fn bytes(sink: &Mutex<Vec<u8>>) -> Vec<u8> {
    sink.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

// 作成時の関門: 実行の公開境界でパイプの詰まり、stderr の欠落、共有した受け取り先を守る。
#[test]
fn input_and_both_output_streams_are_drained_and_may_share_a_sink() -> TestResult {
    let f = ProcessFixture::new()?;
    // 時間の上限を測る検査ではないので、負荷によるパイプの回収の遅れを待つ。
    let settings = RunLimits {
        pipe_close_wait: Duration::from_secs(5),
        ..limits()
    };
    let input = vec![b'x'; 1024 * 1024];
    for shared in [false, true] {
        let stdout = sink();
        let stderr = if shared { Arc::clone(&stdout) } else { sink() };
        let outcome = run(
            command(&f, &["echo-stdin"]),
            input.clone(),
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            settings,
            &AtomicBool::new(false),
        )?;
        assert!(outcome.status.success());
        assert_eq!(outcome.end, RunEnd::Exited);
        assert_eq!(outcome.leftovers, Leftovers::None);
        let out = bytes(&stdout);
        if shared {
            assert_eq!(
                out.iter().filter(|byte| **byte == b'x').count(),
                input.len()
            );
            assert!(
                out.windows(b"echo stderr\n".len())
                    .any(|w| w == b"echo stderr\n")
            );
        } else {
            assert_eq!(out, input);
            assert_eq!(bytes(&stderr), b"echo stderr\n");
        }
    }
    let outcome = run(
        command(&f, &["echo-stdin"]),
        vec![],
        sink(),
        sink(),
        settings,
        &AtomicBool::new(false),
    )?;
    assert!(outcome.status.success());
    // 子が読まない大量の stdin でも、書き込みの完了を待たない。
    let outcome = run(
        command(&f, &["noop"]),
        vec![0; 1024 * 1024],
        sink(),
        sink(),
        settings,
        &AtomicBool::new(false),
    )?;
    assert!(outcome.status.success());
    Ok(())
}

// 作成時の関門: 実際の受け手の準備後に timeout を迎え、指定された合図の違いを守る。
#[test]
fn timeout_forwards_each_stop_signal_and_escalates_when_ignored() -> TestResult {
    for (signal, expected) in [
        (StopSignal::Interrupt, "SIGINT"),
        (StopSignal::Terminate, "SIGTERM"),
    ] {
        for mode in ["handle", "ignore"] {
            let f = ProcessFixture::new()?;
            let mut command = command(&f, &["signal", mode]);
            command.arg(&f.marker);
            let mut settings = limits();
            settings.timeout = Duration::from_millis(800);
            settings.stop_signal = signal;
            let start = Instant::now();
            let outcome = run(
                command,
                vec![],
                sink(),
                sink(),
                settings,
                &AtomicBool::new(false),
            )?;
            let elapsed = start.elapsed();
            assert_eq!(outcome.end, RunEnd::TimedOut);
            assert_eq!(outcome.leftovers, Leftovers::None);
            assert!(elapsed < Duration::from_secs(4));
            if mode == "handle" {
                assert!(outcome.status.success());
                assert_eq!(fs::read_to_string(&f.marker)?, expected);
            } else {
                assert_eq!(fs::read_to_string(&f.marker)?, "ready");
                assert_eq!(outcome.status.signal(), Some(9));
                assert!(elapsed >= settings.timeout.saturating_add(settings.grace));
            }
        }
    }
    Ok(())
}

// 作成時の関門: エージェントが実行中に更新する AtomicBool を、子の終了待ち中も監視する。
#[test]
fn cancellation_is_observed_while_waiting_for_exit() -> TestResult {
    let f = ProcessFixture::new()?;
    let cancelled = AtomicBool::new(false);
    let mut command = command(&f, &["signal", "handle"]);
    command.arg(&f.marker);
    std::thread::scope(|scope| -> TestResult {
        let setter = scope.spawn(|| -> Result<(), String> {
            f.wait_until(|| f.marker.is_file())
                .map_err(|error| error.to_string())?;
            cancelled.store(true, Ordering::Release);
            Ok(())
        });
        let outcome = run(command, vec![], sink(), sink(), limits(), &cancelled)?;
        setter
            .join()
            .map_err(|_| "Cancellation setter panicked")??;
        assert_eq!(outcome.end, RunEnd::Cancelled);
        assert!(outcome.status.success());
        assert_eq!(fs::read_to_string(&f.marker)?, "SIGTERM");
        Ok(())
    })
}

// 作成時の関門: 親が正常に終わった後の子への SIGKILL と、setsid による限界を本物で区別する。
#[test]
fn group_children_die_and_escaped_children_are_detected_only_with_open_pipes() -> TestResult {
    for (session, pipes, expected) in [
        ("group", "open", Leftovers::None),
        ("session", "open", Leftovers::PossiblyRemaining),
        ("session", "closed", Leftovers::None),
    ] {
        let f = ProcessFixture::new()?;
        let mut command = command(&f, &["descendant", session, pipes]);
        command.arg(&f.marker);
        let outcome = run(
            command,
            vec![],
            sink(),
            sink(),
            limits(),
            &AtomicBool::new(false),
        )?;
        let pid = f.pid()?;
        assert_eq!(outcome.end, RunEnd::Exited);
        assert!(outcome.status.success());
        assert_eq!(outcome.leftovers, expected);
        if session == "group" {
            f.wait_until(|| f.stopped(pid))?;
        } else {
            assert!(!f.stopped(pid));
        }
    }
    Ok(())
}

struct BrokenSink;
impl Write for BrokenSink {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("sink failed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// 作成時の関門: 出力先の失敗で読み取りをやめ、子が詰まって timeout する退行を捕まえる。
#[test]
fn failed_output_sink_does_not_stop_draining_and_poisoned_sinks_remain_usable() -> TestResult {
    let f = ProcessFixture::new()?;
    let start = Instant::now();
    let result = run(
        command(&f, &["echo-stdin"]),
        vec![b'x'; 1024 * 1024],
        Arc::new(Mutex::new(BrokenSink)),
        sink(),
        limits(),
        &AtomicBool::new(false),
    );
    assert!(result.is_err());
    assert!(start.elapsed() < Duration::from_secs(4));
    let stdout = sink();
    // 別スレッドの意図した panic で毒を作る。実装に panic の差し込み口は設けない。
    let clone = Arc::clone(&stdout);
    let _joined = std::thread::spawn(move || {
        let _guard = clone.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(std::thread::panicking(), "poison the test sink");
    })
    .join();
    let outcome = run(
        command(&f, &["echo-stdin"]),
        b"poison".to_vec(),
        Arc::clone(&stdout),
        sink(),
        limits(),
        &AtomicBool::new(false),
    )?;
    assert!(outcome.status.success());
    assert_eq!(bytes(&stdout), b"poison");
    Ok(())
}

// P2-01 が求める測定。比較対象は同じ実行ファイルと同じ作業ディレクトリに揃える。
#[test]
#[ignore = "manual timing measurement: run with --ignored --nocapture"]
fn measure_noop_overhead() -> TestResult {
    let f = ProcessFixture::new()?;
    let mut direct = Vec::new();
    let mut managed = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        assert!(command(&f, &["noop"]).output()?.status.success());
        direct.push(start.elapsed());
        let start = Instant::now();
        assert!(
            run(
                command(&f, &["noop"]),
                vec![],
                sink(),
                sink(),
                limits(),
                &AtomicBool::new(false)
            )?
            .status
            .success()
        );
        managed.push(start.elapsed());
    }
    direct.sort();
    managed.sort();
    let median = |values: &[Duration]| -> Result<Duration, Box<dyn std::error::Error>> {
        Ok(values
            .get(9)
            .ok_or("Missing measurement")?
            .saturating_add(*values.get(10).ok_or("Missing measurement")?)
            / 2)
    };
    eprintln!(
        "20-run medians: Command::output={:?}, run={:?}",
        median(&direct)?,
        median(&managed)?
    );
    Ok(())
}

// 端末を切り離す補助一回の増分を、同じ probe・環境・作業場所で測る。
#[test]
#[ignore = "manual session startup measurement; run release with --ignored --nocapture"]
fn measure_session_startup_overhead() -> TestResult {
    let f = ProcessFixture::new()?;
    let mut direct = Vec::new();
    let mut detached = Vec::new();
    for index in 0..110 {
        for wrapped in if index % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let mut launch = if wrapped {
                command(&f, &["noop"])
            } else {
                let mut launch = std::process::Command::new(&f.probe);
                launch.arg("noop").current_dir(&f.root);
                launch
            };
            launch.env_clear();
            let started = Instant::now();
            assert!(launch.output()?.status.success());
            if index >= 10 {
                if wrapped {
                    detached.push(started.elapsed());
                } else {
                    direct.push(started.elapsed());
                }
            }
        }
    }
    direct.sort();
    detached.sort();
    let median = |samples: &[Duration]| -> Result<Duration, Box<dyn std::error::Error>> {
        Ok(*samples.get(49).ok_or("missing median")?)
    };
    let plain = median(&direct)?;
    let session = median(&detached)?;
    eprintln!(
        "100 runs after 10 warmups: direct median={plain:?}, __session median={session:?}, increase={:?}; p90 direct={:?}, __session={:?}",
        session.saturating_sub(plain),
        direct.get(89),
        detached.get(89)
    );
    Ok(())
}

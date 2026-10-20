//! 発行した BPF に模擬の syscall データを渡して CPU・番号・引数の拒否を検証する（実装プラン P0-05「テスト」）。

use super::{compile, compile_no_process};
use seccompiler::BpfProgram;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// 作成時の関門: 生の syscall に unsafe が要るため、作業文書が認めたフィルタの
// 境界で確かめる。条件の逆転、CPU の検査漏れ、x32 の前置の削除を捕まえる。
// 実際の OS の拒否については sandbox_linux の子プロセスのテストが持つ。
fn evaluate(
    program: &BpfProgram,
    arch: u32,
    nr: u32,
    args: [u64; 6],
) -> Result<u32, Box<dyn std::error::Error>> {
    let mut data = Vec::new();
    data.extend(nr.to_le_bytes());
    data.extend(arch.to_le_bytes());
    data.extend(0_u64.to_le_bytes());
    for arg in args {
        data.extend(arg.to_le_bytes());
    }
    let mut accumulator = 0_u32;
    let mut pc = 0_usize;
    for _ in 0..4096 {
        let instruction = program.get(pc).ok_or("BPF ran past the program")?;
        pc = pc.checked_add(1).ok_or("BPF PC overflow")?;
        let offset = match instruction.code {
            0x20 => {
                let offset = usize::try_from(instruction.k)?;
                let end = offset.checked_add(4).ok_or("BPF load overflow")?;
                let bytes: [u8; 4] = data
                    .get(offset..end)
                    .ok_or("BPF load outside data")?
                    .try_into()?;
                accumulator = u32::from_le_bytes(bytes);
                0
            }
            0x54 => {
                accumulator &= instruction.k;
                0
            }
            0x05 => usize::try_from(instruction.k)?,
            0x15 | 0x25 | 0x35 | 0x45 => {
                let condition = match instruction.code {
                    0x15 => accumulator == instruction.k,
                    0x25 => accumulator > instruction.k,
                    0x35 => accumulator >= instruction.k,
                    0x45 => accumulator & instruction.k != 0,
                    _ => return Err("Unexpected BPF jump".into()),
                };
                usize::from(if condition {
                    instruction.jt
                } else {
                    instruction.jf
                })
            }
            0x06 => return Ok(instruction.k),
            _ => return Err(format!("Unsupported BPF opcode {}", instruction.code).into()),
        };
        pc = pc.checked_add(offset).ok_or("BPF jump overflow")?;
    }
    Err("BPF did not terminate".into())
}

fn arch() -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        0xc000_003e
    }
    #[cfg(target_arch = "aarch64")]
    {
        0xc000_00b7
    }
}

#[test]
fn compiled_filter_rejects_bypass_syscalls_and_foreign_architectures() -> TestResult {
    let denied = libc::SECCOMP_RET_ERRNO | u32::try_from(libc::EPERM)?;
    for allow_ip in [false, true] {
        let filter = compile(allow_ip)?;
        for syscall in [
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
        ] {
            assert_eq!(
                evaluate(&filter, arch(), u32::try_from(syscall)?, [0; 6])?,
                denied
            );
        }
        assert_eq!(
            evaluate(&filter, arch(), u32::try_from(libc::SYS_getpid)?, [0; 6])?,
            libc::SECCOMP_RET_ALLOW
        );
        // i386 の AUDIT_ARCH_I386。x86_64 の socketcall への切り替えもここで拒まれる。
        assert_eq!(
            evaluate(&filter, 0x4000_0003, 102, [0; 6])?,
            libc::SECCOMP_RET_KILL_PROCESS
        );
        #[cfg(target_arch = "x86_64")]
        for syscall in [libc::SYS_socket, libc::SYS_socketpair, libc::SYS_getpid] {
            assert_eq!(
                evaluate(
                    &filter,
                    arch(),
                    u32::try_from(syscall)? | 0x4000_0000,
                    [0; 6]
                )?,
                denied
            );
        }
    }
    Ok(())
}

// 作成時の関門: musl の子起動で通る clone 以外の番号や互換 ABI の拒否を落とす退行を捕まえる。
// 既存の評価器で発行した BPF の動作を確かめ、画像補助の子テストは実際の OS の拒否を持つ。
#[test]
fn compiled_image_filter_rejects_process_syscalls_and_compatible_abis() -> TestResult {
    let filter = compile_no_process()?;
    let denied = libc::SECCOMP_RET_ERRNO | u32::try_from(libc::EPERM)?;
    let process_syscalls = [
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_execve,
        libc::SYS_execveat,
    ]
    .into_iter();
    #[cfg(target_arch = "x86_64")]
    let process_syscalls = process_syscalls.chain([libc::SYS_fork, libc::SYS_vfork]);
    for syscall in process_syscalls {
        assert_eq!(
            evaluate(&filter, arch(), u32::try_from(syscall)?, [0; 6])?,
            denied,
            "syscall {syscall}"
        );
    }
    for syscall in [
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_mmap,
        libc::SYS_exit_group,
    ] {
        assert_eq!(
            evaluate(&filter, arch(), u32::try_from(syscall)?, [0; 6])?,
            libc::SECCOMP_RET_ALLOW,
            "syscall {syscall}"
        );
        #[cfg(target_arch = "x86_64")]
        assert_eq!(
            evaluate(
                &filter,
                arch(),
                u32::try_from(syscall)? | 0x4000_0000,
                [0; 6]
            )?,
            denied,
            "x32 syscall {syscall}"
        );
    }
    // i386 の fork・execve・socketcall。ネイティブの番号と解釈して許してはならない。
    #[cfg(target_arch = "x86_64")]
    for syscall in [2, 11, 102] {
        assert_eq!(
            evaluate(&filter, 0x4000_0003, syscall, [0; 6])?,
            libc::SECCOMP_RET_KILL_PROCESS,
            "i386 syscall {syscall}"
        );
    }
    Ok(())
}

#[test]
fn compiled_socket_rules_limit_domains_types_and_ip_operations() -> TestResult {
    let denied = libc::SECCOMP_RET_ERRNO | u32::try_from(libc::EPERM)?;
    for allow_ip in [false, true] {
        let filter = compile(allow_ip)?;
        for domain in [
            libc::AF_UNIX,
            libc::AF_INET,
            libc::AF_INET6,
            libc::AF_NETLINK,
            libc::AF_PACKET,
        ] {
            let mut args = [0; 6];
            *args.get_mut(0).ok_or("missing domain")? = u64::try_from(domain)?;
            let expected = if allow_ip && [libc::AF_INET, libc::AF_INET6].contains(&domain) {
                libc::SECCOMP_RET_ALLOW
            } else {
                denied
            };
            assert_eq!(
                evaluate(&filter, arch(), u32::try_from(libc::SYS_socket)?, args)?,
                expected
            );
            for kind in [
                libc::SOCK_STREAM,
                libc::SOCK_SEQPACKET,
                libc::SOCK_DGRAM,
                libc::SOCK_RAW,
            ] {
                for flags in [
                    0,
                    libc::SOCK_CLOEXEC,
                    libc::SOCK_NONBLOCK,
                    libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                    0x2000_0000,
                ] {
                    *args.get_mut(1).ok_or("missing type")? = u64::try_from(kind | flags)?;
                    let allowed = domain == libc::AF_UNIX
                        && [libc::SOCK_STREAM, libc::SOCK_SEQPACKET].contains(&kind)
                        && flags != 0x2000_0000;
                    assert_eq!(
                        evaluate(&filter, arch(), u32::try_from(libc::SYS_socketpair)?, args)?,
                        if allowed {
                            libc::SECCOMP_RET_ALLOW
                        } else {
                            denied
                        },
                        "domain={domain} kind={kind} flags={flags}"
                    );
                }
            }
        }
        for syscall in [
            libc::SYS_connect,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_accept4,
        ] {
            assert_eq!(
                evaluate(&filter, arch(), u32::try_from(syscall)?, [0; 6])?,
                if allow_ip {
                    libc::SECCOMP_RET_ALLOW
                } else {
                    denied
                }
            );
        }
        #[cfg(target_arch = "x86_64")]
        assert_eq!(
            evaluate(&filter, arch(), u32::try_from(libc::SYS_accept)?, [0; 6])?,
            if allow_ip {
                libc::SECCOMP_RET_ALLOW
            } else {
                denied
            }
        );
    }
    Ok(())
}

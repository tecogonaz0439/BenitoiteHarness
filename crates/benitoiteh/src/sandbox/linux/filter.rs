//! CPU の種類・ソケット・画像補助のプロセス作成を制限する seccomp を作る（設計書 03-02「方針の要素」、01-03「画像のファイルの読み取り」）。

use std::collections::BTreeMap;

#[cfg(target_arch = "x86_64")]
use seccompiler::sock_filter;
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};

type Result<T> = std::result::Result<T, seccompiler::BackendError>;

/// 呼び出し元が検証済みでも、表現できない接続先の制限をここでも拒む。
pub(super) fn compile_policy(
    config: &super::super::PolicyConfig,
) -> std::result::Result<BpfProgram, super::super::SandboxError> {
    super::super::policy::reject_loopback_port(config)?;
    compile(config.allow_ip)
        .map_err(|error| super::super::SandboxError::Io(std::io::Error::other(error)))
}

fn condition(argument: u8, comparison: SeccompCmpOp, value: i32) -> Result<SeccompCondition> {
    // カーネル側の int 引数と同じ下位 32 ビットを比べる。上位ビットで拒否を回り込ませない。
    SeccompCondition::new(
        argument,
        SeccompCmpArgLen::Dword,
        comparison,
        value as u32 as u64,
    )
}

pub(super) fn compile(allow_ip: bool) -> Result<BpfProgram> {
    // 一致した呼び出しを拒む。socketpair は領域が異なるか、種類が許す 8 通りの
    // どれとも一致しない場合を拒む条件にする。MaskedEq に「等しくない」はないので、
    // CLOEXEC と NONBLOCK の組み合わせを列挙する（実装プラン P0-05「作るもの」2）。
    let mut rules = BTreeMap::new();
    for syscall in [
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
    ] {
        rules.insert(syscall, vec![]);
    }
    if allow_ip {
        rules.insert(
            libc::SYS_socket,
            vec![SeccompRule::new(vec![
                condition(0, SeccompCmpOp::Ne, libc::AF_INET)?,
                condition(0, SeccompCmpOp::Ne, libc::AF_INET6)?,
            ])?],
        );
    } else {
        rules.insert(libc::SYS_socket, vec![]);
        for syscall in [
            libc::SYS_connect,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_accept4,
        ] {
            rules.insert(syscall, vec![]);
        }
        // aarch64 の番号の表は accept を持たず、accept4 だけを持つ。
        #[cfg(target_arch = "x86_64")]
        rules.insert(libc::SYS_accept, vec![]);
    }
    let mut invalid_types = Vec::new();
    for kind in [libc::SOCK_STREAM, libc::SOCK_SEQPACKET] {
        for flags in [
            0,
            libc::SOCK_CLOEXEC,
            libc::SOCK_NONBLOCK,
            libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
        ] {
            invalid_types.push(condition(1, SeccompCmpOp::Ne, kind | flags)?);
        }
    }
    rules.insert(
        libc::SYS_socketpair,
        vec![
            SeccompRule::new(vec![condition(0, SeccompCmpOp::Ne, libc::AF_UNIX)?])?,
            SeccompRule::new(invalid_types)?,
        ],
    );
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into()?,
    )?;
    let compiled: BpfProgram = filter.try_into()?;
    Ok(reject_x32(compiled))
}

/// 画像補助が root でも、子とスレッドの作成とプログラムの起動を拒む。
pub(crate) fn compile_no_process() -> Result<BpfProgram> {
    let mut rules = BTreeMap::new();
    for syscall in [
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_execve,
        libc::SYS_execveat,
    ] {
        rules.insert(syscall, vec![]);
    }
    // aarch64 は fork・vfork の番号を持たない。
    #[cfg(target_arch = "x86_64")]
    for syscall in [libc::SYS_fork, libc::SYS_vfork] {
        rules.insert(syscall, vec![]);
    }
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into()?,
    )?;
    Ok(reject_x32(filter.try_into()?))
}

fn reject_x32(compiled: BpfProgram) -> BpfProgram {
    #[cfg(target_arch = "x86_64")]
    {
        // x32 は AUDIT_ARCH_X86_64 を共有するので、ライブラリの CPU の検査だけでは
        // 足りない。番号のビットを先に拒む（実装プラン P0-05「作るもの」2）。
        let mut prefix = vec![
            sock_filter {
                code: 0x20,
                jt: 0,
                jf: 0,
                k: 0,
            }, // LD W ABS nr
            sock_filter {
                code: 0x45,
                jt: 0,
                jf: 1,
                k: 0x4000_0000,
            }, // JMP JSET K
            sock_filter {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
            },
        ];
        prefix.extend(compiled);
        prefix
    }
    #[cfg(not(target_arch = "x86_64"))]
    compiled
}

#[cfg(test)]
mod tests;

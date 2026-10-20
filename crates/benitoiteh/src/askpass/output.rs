//! 秘密の一致範囲を重ねて集め、切り詰めの前に出力を伏せる（設計書 03-04「段 2」）。
use super::{CallState, MAX_SECRET, text};
use sha2::{Digest, Sha256};
use std::sync::{Arc, PoisonError};
use zeroize::Zeroizing;

/// 一つの秘密の照合の値。短い秘密は SHA-256 から総当たりで求めうるので、
/// 手放すときは `Zeroizing` の入れ物で 0 に上書きする（設計書 03-04「秘密の扱い」）。
#[derive(Clone, Copy, Default)]
pub(super) struct SecretHash {
    length: usize,
    sha256: [u8; 32],
    rolling: u64,
    power: u64,
}
impl zeroize::DefaultIsZeroes for SecretHash {}

/// 一つの呼び出しで出せる求めの上限。照合の値の入れ物はこの容量で作り、伸ばさない
/// （伸ばすと古い領域に写しが残る）。
pub(super) const MAX_HASHES: usize = 3;
pub(super) fn hash_list() -> Zeroizing<Vec<SecretHash>> {
    Zeroizing::new(Vec::with_capacity(MAX_HASHES))
}

/// 照合で区切りとして扱うバイト。秘密はこれらを含まないので、一致はこれらをまたがない。
fn separator(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r' | 0)
}

impl SecretHash {
    /// 区切りのバイトを含む秘密には照合の値を作らない（区切りの手前まで渡す論証が崩れるため）。
    pub(super) fn new(bytes: &[u8], base: u64) -> Option<Self> {
        if bytes.iter().any(|byte| separator(*byte)) {
            return None;
        }
        let mut rolling = 0u64;
        let mut power = 1u64;
        for byte in bytes {
            rolling = rolling
                .wrapping_mul(base)
                .wrapping_add(u64::from(*byte) + 1);
            power = power.wrapping_mul(base);
        }
        Some(Self {
            length: bytes.len(),
            sha256: Sha256::digest(bytes).into(),
            rolling,
            power,
        })
    }
}

/// 未確定の末尾だけを保持する。返すバイトは既に伏せてあり、末尾の上限を掛けてよい。
pub(crate) struct Redactor {
    call: Arc<CallState>,
    /// 呼び出しの状態から写した照合の値。写しも手放すときに消す。
    hashes: Zeroizing<Vec<SecretHash>>,
    pending: Zeroizing<Vec<u8>>,
    covered: Vec<bool>,
    prefixes: Zeroizing<Vec<u64>>,
    #[cfg(test)]
    prefix_steps: usize,
    in_secret: bool,
    #[cfg(test)]
    sha_checks: usize,
}
impl Redactor {
    pub(super) fn new(call: Arc<CallState>) -> Self {
        // 保留の上限と一回の読み取りに足る容量を先に取り、指紋の再確保を避ける。
        let mut prefixes = Zeroizing::new(Vec::with_capacity(MAX_SECRET.saturating_add(8193)));
        prefixes.push(0);
        Self {
            call,
            hashes: hash_list(),
            pending: Zeroizing::new(Vec::with_capacity(MAX_SECRET.saturating_add(8192))),
            covered: Vec::with_capacity(MAX_SECRET.saturating_add(8192)),
            prefixes,
            #[cfg(test)]
            prefix_steps: 0,
            in_secret: false,
            #[cfg(test)]
            sha_checks: 0,
        }
    }
    pub(crate) fn push(&mut self, bytes: &[u8], finish: bool) -> Vec<u8> {
        // 錠の中では新しく増えた指紋だけ写す。出力の走査で回答を止めない。
        // 写す先は容量 3 で作ってあり、伸ばさないので古い領域に写しを残さない。
        {
            let state = self
                .call
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for hash in state.hashes.iter().skip(self.hashes.len()) {
                if self.hashes.len() < MAX_HASHES {
                    self.hashes.push(*hash);
                }
            }
        }
        // 登録前の出力は秘密を含まない。走査も未確定の末尾の保持も要らない。
        if self.hashes.is_empty() {
            return bytes.to_vec();
        }
        self.pending.extend_from_slice(bytes);
        self.covered.resize(self.pending.len(), false);
        let base = *self.call.rolling_base;
        // 末尾の接頭辞は書き込みをまたいで残す。基準を引き直す必要はない。
        // 指紋の途中の値から入力を復元できるため、この作業領域も消す。
        let mut prefix = self.prefixes.last().copied().unwrap_or(0);
        for byte in bytes {
            prefix = prefix.wrapping_mul(base).wrapping_add(u64::from(*byte) + 1);
            self.prefixes.push(prefix);
            #[cfg(test)]
            {
                self.prefix_steps = self.prefix_steps.saturating_add(1);
            }
        }
        // 呼び出し固有の乱数の基数で候補を絞る。一致候補だけ SHA-256 で確かめる。
        // 指紋の衝突は伏せ字の漏れにも誤った一致にも使わない。
        for hash in self.hashes.iter() {
            if hash.length == 0 {
                continue;
            }
            for offset in 0..self.pending.len() {
                let end = offset.saturating_add(hash.length);
                let (Some(start), Some(end_hash)) =
                    (self.prefixes.get(offset), self.prefixes.get(end))
                else {
                    break;
                };
                if end_hash.wrapping_sub(start.wrapping_mul(hash.power)) != hash.rolling {
                    continue;
                }
                if let Some(window) = self.pending.get(offset..end) {
                    #[cfg(test)]
                    {
                        self.sha_checks = self.sha_checks.saturating_add(1);
                    }
                    let digest: [u8; 32] = Sha256::digest(window).into();
                    if digest == hash.sha256
                        && let Some(covered) = self.covered.get_mut(offset..end)
                    {
                        covered.fill(true);
                    }
                }
            }
        }
        let lookbehind = self
            .hashes
            .iter()
            .map(|hash| hash.length)
            .max()
            .unwrap_or(0)
            .saturating_sub(1);
        // 秘密は区切りを含まないので、最後の区切りまでで終わる一致もそこをまたぐ一致もない。
        // そこまでは留めずに渡し、行の終わりの出力と混ぜた並びの順を遅らせない
        // （設計書 03-04「秘密の扱い」）。
        let count = if finish {
            self.pending.len()
        } else {
            let boundary = self
                .pending
                .iter()
                .rposition(|byte| separator(*byte))
                .map_or(0, |index| index.saturating_add(1));
            self.pending.len().saturating_sub(lookbehind).max(boundary)
        };
        let mut output = Vec::new();
        for (byte, covered) in self.pending.iter().zip(&self.covered).take(count) {
            if *covered {
                if !self.in_secret {
                    output.extend_from_slice(text::REPLACEMENT);
                }
                self.in_secret = true;
            } else {
                output.push(*byte);
                self.in_secret = false;
            }
        }
        // 重なりが次の読み込みまで続く場合も、範囲の印を残す。
        self.pending.drain(..count);
        self.covered.drain(..count);
        self.prefixes.drain(..count);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    // 関門: 登録前の出力を保持せず、その出力を後から秘密として扱わない。
    #[test]
    fn output_before_secret_registration_is_immediate_and_unbuffered() -> std::io::Result<()> {
        let f = crate::askpass::tests::Fixture::new()?;
        let mut redactor = f.call.redactor();
        assert_eq!(redactor.push(b"before", false), b"before");
        assert!(redactor.pending.is_empty());
        assert_eq!(redactor.prefix_steps, 0);
        assert_eq!(redactor.prefixes.len(), 1);
        let requests = f.broker.take_requests().ok_or(std::io::ErrorKind::Other)?;
        let mut stream = f.request(None, "synthetic")?;
        requests
            .recv()
            .map_err(std::io::Error::other)?
            .reply
            .send(crate::askpass::Answer::Secret(Zeroizing::new(
                "before".into(),
            )))
            .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
        stream.read_to_end(&mut Vec::new())?;
        let mut output = Vec::new();
        for byte in b"before before" {
            output.extend(redactor.push(std::slice::from_ref(byte), false));
        }
        output.extend(redactor.push(&[], true));
        assert_eq!(output, b"[secret] [secret]");
        // 各入力バイトの接頭辞は一度だけ作り、保留した 5 バイトを計算し直さない。
        assert_eq!(redactor.prefix_steps, b"before before".len());
        assert!(redactor.pending.is_empty());
        assert_eq!(redactor.prefixes.len(), 1);
        Ok(())
    }

    // 時間でなく SHA-256 の候補検査の回数を守る。出力の大半が非一致でも
    // 長い秘密の全バイトを各位置でハッシュする退行を捕まえる。
    #[test]
    fn long_secrets_filter_non_candidates_without_sha256_per_window() -> std::io::Result<()> {
        let f = crate::askpass::tests::Fixture::new()?;
        let requests = f.broker.take_requests().ok_or(std::io::ErrorKind::Other)?;
        for suffix in ['a', 'b', 'c'] {
            let secret = format!("{}{suffix}", "z".repeat(MAX_SECRET.saturating_sub(1)));
            let mut stream = f.request(None, "synthetic")?;
            requests
                .recv()
                .map_err(std::io::Error::other)?
                .reply
                .send(crate::askpass::Answer::Secret(Zeroizing::new(secret)))
                .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
            stream.read_to_end(&mut Vec::new())?;
        }
        let mut redactor = f.call.redactor();
        let chunk = vec![b'z'; 8192];
        let mut output = Vec::new();
        for _ in 0..128 {
            output.extend(redactor.push(&chunk, false));
            assert_eq!(redactor.sha_checks, 0);
        }
        output.extend(redactor.push(&[], true));
        assert_eq!(output, vec![b'z'; 1024 * 1024]);
        // 最後の一文字だけが違うので、乱数の基数によらず候補にならない。
        assert_eq!(redactor.sha_checks, 0);
        Ok(())
    }
}

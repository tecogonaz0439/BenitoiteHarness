//! 私的なネットワークと特殊用途のアドレスを拒む（設計書 01-03「web_fetch」）。

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// 宛先にしてよいアドレスか。種類で私的・特殊用途のものを拒み、さらに機械自身の口に付いたアドレス
/// （Mac の大域の IPv6、VPS の公開の IPv4 など）も拒む。後者を通すと、攻撃者が DNS の向き先を機械自身へ
/// 替えて、すべての口で待ち受けるサンドボックスの外のサービスに届かせられる（設計書 01-03「web_fetch」、
/// 03-02「通信の中継」）。ルータの公開のアドレスへの折り返し（NAT のヘアピン）は見分けられない。
pub(crate) fn allowed(address: IpAddr) -> bool {
    kind_allowed(address) && !assigned_here(address.to_canonical())
}

/// 機械自身の口に付いたアドレスか。付いていれば、そのアドレスで bind できる。`unsafe` で口の一覧を
/// 読まずに、標準のライブラリだけで判定する。
pub(super) fn assigned_here(address: IpAddr) -> bool {
    UdpSocket::bind(SocketAddr::new(address, 0)).is_ok()
}

fn kind_allowed(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => ipv4(ip),
        IpAddr::V6(ip) => {
            let [a, b, c, d, e, f, g, h] = ip.segments();
            if ip.is_unspecified() || ip.is_loopback() {
                return false;
            }
            // ::1 を IPv4 互換と誤認しないよう、特別扱いの後に上位 96 ビットを見る。
            if a == 0 && b == 0 && c == 0 && d == 0 && e == 0 && (f == 0 || f == 0xffff) {
                return embedded(g, h);
            }
            if a == 0x64 && b == 0xff9b && c == 0 && d == 0 && e == 0 && f == 0 {
                return embedded(g, h);
            }
            // 網内の 64:ff9b:1::/48 は IPv4 の位置を一つに決められないため、
            // 2000::/3 の外として拒む。NAT64 の例外は上の /96 だけにする。
            (a & 0xe000) == 0x2000 && a != 0x2002 && !(a == 0x2001 && (b == 0 || b == 0xdb8))
        }
    }
}

fn embedded(high: u16, low: u16) -> bool {
    let [a, b] = high.to_be_bytes();
    let [c, d] = low.to_be_bytes();
    ipv4(Ipv4Addr::new(a, b, c, d))
}

fn ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || (a == 198 && (b == 18 || b == 19)))
}

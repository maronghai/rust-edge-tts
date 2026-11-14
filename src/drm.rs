//! Request signing compatible with edge-tts Sec-MS-GEC v2.
use std::sync::atomic::{AtomicI64, Ordering};
/// Protocol client token used by the service signing algorithm; not a user credential.
pub const TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// Signature protocol/browser version sent in requests.
pub const VERSION: &str = "2-143.0.3650.75";
static SKEW: AtomicI64 = AtomicI64::new(0);
const INITIAL: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];
const STATE: [u32; 8] = [
    0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A, 0x6C006F49, 0x04BD35AB, 0xFEB0A7E4, 0x03EFFBD2,
];
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
fn s0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}
fn s1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}
fn expand(w: &mut Vec<u32>) {
    for i in w.len()..64 {
        w.push(
            s1(w[i - 2])
                .wrapping_add(w[i - 7])
                .wrapping_add(s0(w[i - 15]))
                .wrapping_add(w[i - 16]),
        );
    }
}
fn rounds(state: [u32; 8], w: &[u32], start: usize) -> [u32; 8] {
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
    for i in start..64 {
        let k = if start == 14 && i == 14 {
            0xC9DC06A7
        } else {
            K[i]
        };
        let t1 = h
            .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
            .wrapping_add(g ^ (e & (f ^ g)))
            .wrapping_add(k)
            .wrapping_add(w[i]);
        let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
            .wrapping_add((a & b) | (c & (a | b)));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    [a, b, c, d, e, f, g, h]
}
fn digest(data: &[u8]) -> Vec<u8> {
    let schedule = [
        0xEB24BF9Eu32,
        0x1558691F,
        0xA0C09EE2,
        0x0CFA32BA,
        0xA622EE8C,
        0xD1DD9491,
        0xC0DD25A0,
        0xF9E70AB6,
        0xF0C31573,
        0x7058CE4E,
        0x7188E606,
        0x00E5AB33,
        0xD258B060,
        0x72766564,
    ];
    let mut w = vec![0; 14];
    w.push(u32::from_be_bytes([0, data[0], data[1], data[2]]));
    w.push(u32::from_be_bytes(data[3..7].try_into().unwrap()));
    for i in 16..30 {
        let mut v = schedule[i - 16];
        if i - 2 >= 14 {
            v = v.wrapping_add(s1(w[i - 2]) ^ if i == 16 { 0x000B92C0 } else { 0 });
        }
        if i - 7 >= 14 {
            v = v.wrapping_add(w[i - 7]);
        }
        if i - 15 >= 14 {
            v = v.wrapping_add(s0(w[i - 15]) ^ if i == 29 { 0x059C0B80 } else { 0 });
        }
        w.push(v);
    }
    w.push(
        s1(w[28])
            .wrapping_add(w[23])
            .wrapping_add(s0(w[15]))
            .wrapping_add(w[14])
            .wrapping_add(0xC9DC06A7)
            .wrapping_sub(K[14]),
    );
    expand(&mut w);
    let mut state = rounds(STATE, &w, 14);
    for i in 0..8 {
        state[i] = state[i].wrapping_add(INITIAL[i]);
    }
    let length = 57 + data.len();
    let mut padded = data[7..].to_vec();
    padded.push(0x80);
    padded.extend(vec![0; (119 - length % 64) % 64]);
    padded.extend_from_slice(&((length as u64) * 8).to_be_bytes());
    for block in padded.chunks_exact(64) {
        let mut w = block
            .chunks_exact(4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .collect();
        expand(&mut w);
        let work = rounds(state, &w, 0);
        for i in 0..8 {
            state[i] = state[i].wrapping_add(work[i]);
        }
    }
    state.iter().flat_map(|v| v.to_be_bytes()).collect()
}
fn cs64(data: &[u8], hash: &[u8]) -> [u32; 2] {
    let coeff = [
        (0xE82E91B1u32, 0xB32700D0u32),
        (0x44DE24C6, 0x88DFF51D),
        (0x3BEB65BC, 0x29514038),
        (0xC89C5933, 0xB328CBDF),
        (0x8D1EC211, 0xD405B4D5),
        (0x314EF5D5, 0x26E55F3C),
        (0xE92491B1, 0x607801C6),
        (0xF0A5B9D5, 0x82B2C1FB),
        (0x06064CF3, 0xFA51A3DA),
        (0xDA79FA60, 0x207CC464),
        (0xBED20560, 0x3C3C74F3),
        (0x89F9C9F8, 0xD2D14B61),
        (0x35FA38C6, 0xBB04A2C1),
        (0xCB49E3C6, 0xC5626167),
    ];
    let a = u32::from_le_bytes(hash[..4].try_into().unwrap()) | 1;
    let b = u32::from_le_bytes(hash[4..8].try_into().unwrap()) | 1;
    let factors = [a.wrapping_mul(0x82893933), b.wrapping_mul(0x82893933)];
    let mut terms: Vec<u32> = coeff
        .iter()
        .enumerate()
        .map(|(i, (c, _))| c.wrapping_mul(factors[i % 2]))
        .collect();
    let mut offsets: Vec<u32> = coeff.iter().map(|(_, v)| *v).collect();
    let mut padded = vec![0];
    padded.extend_from_slice(data);
    while padded.len() % 8 != 0 {
        padded.push(0);
    }
    for (i, word) in padded.chunks_exact(4).enumerate() {
        let x = u32::from_le_bytes(word.try_into().unwrap());
        let mut term = x.wrapping_mul(if i % 2 == 0 { a } else { b });
        let mut offset =
            x.rotate_left(16)
                .wrapping_mul(if i % 2 == 0 { 0xEF0569FB } else { 0xC31713DB });
        if i == 0 {
            term = term.wrapping_add(0xF2F90B1Au32.wrapping_mul(factors[0]));
            offset = offset.wrapping_add(0x0B1A0000);
        }
        terms.push(term);
        offsets.push(offset);
    }
    let (mut swap_acc, mut swap_last, mut state, mut acc) = (0u32, 0u32, 0u32, 0u32);
    for i in (0..terms.len()).step_by(2) {
        let (x, y) = (terms[i], terms[i + 1]);
        let q = offsets[i]
            .wrapping_add(x)
            .rotate_left(16)
            .wrapping_mul(0x76F50F34);
        swap_acc =
            swap_acc.wrapping_add(q.rotate_left(16).wrapping_mul(0xC3EFEA97).wrapping_add(q));
        let q = offsets[i + 1]
            .wrapping_add(y)
            .rotate_left(16)
            .wrapping_mul(0x1881CE3C);
        swap_last = q.rotate_left(16).wrapping_mul(0x35BD1EC9).wrapping_add(q);
        swap_acc = swap_acc.wrapping_add(swap_last);
        state = state.wrapping_mul(a).wrapping_add(x);
        for k in [0xCF98B111, 0x87085B9F, 0x12CEB96D, 0x257E1D83] {
            state = state.rotate_left(16).wrapping_mul(k);
        }
        acc = acc.wrapping_add(state);
        state = state.wrapping_mul(b).wrapping_add(y);
        for k in [0xA27416F5, 0xD38396FF, 0x7C932B89, 0xBFA49F69] {
            state = state.rotate_left(16).wrapping_mul(k);
        }
        acc = acc.wrapping_add(state);
    }
    [swap_last ^ state, swap_acc ^ acc]
}
/// Generate a deterministic Sec-MS-GEC v2 signature for Unix seconds.
///
/// Uses a five-minute timestamp bucket. Does not apply learned server clock
/// skew; use [`token()`] for live requests. Intended for normal service-era
/// timestamps, not arbitrary extreme `i64` values.
///
/// # Panics
/// Extreme timestamps can overflow internal arithmetic in checked builds.
///
/// ```
/// assert_eq!(edge_tts::drm::token_at(1_700_000_000),
///     edge_tts::drm::token_at(1_700_000_001));
/// ```
pub fn token_at(unix_seconds: i64) -> String {
    let seconds = unix_seconds + 11_644_473_600;
    let ticks = (seconds - seconds.rem_euclid(300)) as u64 * 10_000_000;
    let data = format!("{ticks}{TOKEN}");
    let hash = digest(data.as_bytes());
    let cs = cs64(data.as_bytes(), &hash);
    let hex = |b: Vec<u8>| b.iter().map(|x| format!("{x:02X}")).collect::<String>();
    format!(
        "{}-{}",
        hex(cs.iter().flat_map(|v| v.to_le_bytes()).collect()),
        hex(hash)
    )
}
/// Generate a live signature from UTC time plus the process-wide learned skew.
/// The skew is updated internally after an eligible HTTP 403; OS time is unchanged.
pub fn token() -> String {
    token_at(chrono::Utc::now().timestamp() + SKEW.load(Ordering::Relaxed))
}
pub(crate) fn correct(date: Option<&str>) -> crate::Result<()> {
    let date =
        date.ok_or_else(|| crate::Error::Protocol("403 response has no Date header".into()))?;
    let server =
        httpdate::parse_http_date(date).map_err(|e| crate::Error::Protocol(e.to_string()))?;
    let seconds = server
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| crate::Error::Protocol(e.to_string()))?
        .as_secs() as i64;
    SKEW.store(seconds - chrono::Utc::now().timestamp(), Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upstream_v2_vectors() {
        for (timestamp, expected) in [
            (
                0,
                "52BC7A17B50DFEC9-28C4394775C8751CD2F6859C030C06DB1489ACAB3B3A7651831FC26895177ED6",
            ),
            (
                1_700_000_000,
                "2CD23A2CA0B2595A-F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2",
            ),
            (
                1_800_000_000,
                "99E54519B490BE5C-E162DB0977FBDCB9B86DD8148BF1BCDC3C0303176CE39852A42B415935C20654",
            ),
        ] {
            assert_eq!(token_at(timestamp), expected);
        }
    }
}

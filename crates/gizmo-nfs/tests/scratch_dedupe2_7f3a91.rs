//! Throwaway v2: distinct quantities in 0x00034600, with proper string handling and
//! block-level duplicate detection.

use std::collections::BTreeMap;
use std::fmt::Write as _;

const REC: usize = 0x890;
const NL: usize = REC / 4;
const N: usize = 46;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("NFSU2_ROOT").expect("NFSU2_ROOT"))
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}
fn cstr(b: &[u8], off: usize, max: usize) -> String {
    let s = &b[off..(off + max).min(b.len())];
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

struct Data {
    names: Vec<String>,
    recs: Vec<Vec<u8>>,
}

fn load() -> Data {
    let bytes = std::fs::read(root().join("GLOBAL/GLOBALB.BUN")).expect("read");
    let mut o = 0usize;
    let start = loop {
        assert!(o + 8 <= bytes.len(), "chunk not found");
        if u32_at(&bytes, o) == 0x0003_4600 {
            let sz = u32_at(&bytes, o + 4) as usize;
            if o + 8 + sz <= bytes.len() && sz == 8 + N * REC {
                break o + 16;
            }
        }
        o += 4;
    };
    let mut recs = Vec::new();
    let mut names = Vec::new();
    for i in 0..N {
        let r = bytes[start + i * REC..start + (i + 1) * REC].to_vec();
        names.push(cstr(&r, 0, 0x10));
        recs.push(r);
    }
    Data { names, recs }
}

/// STRING fields: byte 0..0x10, 0x20..0x30, 0x40..0x60, 0x60..0x80, 0xC0..0xD0
fn is_string_lane(l: usize) -> bool {
    let o = l * 4;
    (o < 0x10) || (0x20..0x30).contains(&o) || (0x40..0x80).contains(&o) || (0xC0..0xD0).contains(&o)
}

fn decoded_ranges() -> Vec<(usize, usize, &'static str)> {
    let mut v: Vec<(usize, usize, &'static str)> = vec![
        (0x220, 0x224, "mass"),
        (0x224, 0x230, "body_LWH"),
        (0x300, 0x30C, "rpm"),
        (0x310, 0x334, "torque9"),
    ];
    for i in 0..4 {
        let b = 0x120 + i * 0x30;
        v.push((b + 0x00, b + 0x04, "wheel_foreaft"));
        v.push((b + 0x08, b + 0x0C, "wheel_ride"));
        v.push((b + 0x10, b + 0x14, "wheel_radius"));
        v.push((b + 0x14, b + 0x18, "wheel_tyrew"));
        v.push((b + 0x1C, b + 0x20, "wheel_lateral"));
    }
    for &g in &[0x2C0usize, 0x460, 0x4A0, 0x4E0] {
        v.push((g + 0x08, g + 0x0C, "final_drive"));
        v.push((g + 0x10, g + 0x14, "rear_drive"));
        v.push((g + 0x18, g + 0x1C, "gear_count"));
        v.push((g + 0x20, g + 0x40, "gear_ratios"));
    }
    v
}
fn lane_decoded(l: usize) -> Option<&'static str> {
    let o = l * 4;
    decoded_ranges().into_iter().find(|&(a, b, _)| o < b && o + 4 > a).map(|(_, _, n)| n)
}

fn ratio(a: &[f32], b: &[f32]) -> Option<f32> {
    let mut k: Option<f32> = None;
    let mut nz = 0;
    for i in 0..a.len() {
        if !a[i].is_finite() || !b[i].is_finite() {
            return None;
        }
        if b[i].abs() < 1e-12 {
            if a[i].abs() > 1e-9 {
                return None;
            }
            continue;
        }
        nz += 1;
        let r = a[i] / b[i];
        match k {
            None => k = Some(r),
            Some(k0) => {
                if (r - k0).abs() > 1e-4 * k0.abs().max(1e-6) {
                    return None;
                }
            }
        }
    }
    let k = k?;
    if nz < 20 || k.abs() < 1e-9 {
        return None;
    }
    Some(k)
}

fn looks_float(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite() && (*x == 0.0 || (x.abs() > 1e-8 && x.abs() < 1e9)))
}
fn fmt_offs(v: &[usize]) -> String {
    v.iter().map(|&x| format!("{:#06x}", x * 4)).collect::<Vec<_>>().join(" ")
}
fn car(d: &Data, n: &str) -> usize {
    d.names.iter().position(|x| x == n).unwrap_or(0)
}

#[test]
fn lens2() {
    let d = load();
    let mut o = String::new();
    macro_rules! p { ($($t:tt)*) => { { writeln!(o, $($t)*).unwrap(); } } }
    let sx = car(&d, "240SX");

    let mut bits: Vec<Vec<u32>> = Vec::with_capacity(NL);
    let mut fs: Vec<Vec<f32>> = Vec::with_capacity(NL);
    for l in 0..NL {
        let off = l * 4;
        bits.push(d.recs.iter().map(|r| u32_at(r, off)).collect());
        fs.push(d.recs.iter().map(|r| f32_at(r, off)).collect());
    }

    // ---------- classification ----------
    let mut kind: Vec<&'static str> = vec![""; NL];
    let (mut nstr, mut ndec, mut nzero, mut nconst) = (0, 0, 0, 0);
    let mut varying: Vec<usize> = Vec::new();
    for l in 0..NL {
        if is_string_lane(l) {
            kind[l] = "string";
            nstr += 1;
        } else if lane_decoded(l).is_some() {
            kind[l] = "decoded";
            ndec += 1;
        } else if bits[l].iter().all(|&x| x == 0) {
            kind[l] = "zero";
            nzero += 1;
        } else if bits[l].iter().all(|&x| x == bits[l][0]) {
            kind[l] = "const";
            nconst += 1;
        } else {
            kind[l] = "vary";
            varying.push(l);
        }
    }
    p!("== coverage of 548 lanes ==");
    p!("string {nstr} | decoded {ndec} | zero {nzero} | const {nconst} | VARY {}", varying.len());

    // non-float varying lanes (ints / bitfields)
    let (intish, varying): (Vec<usize>, Vec<usize>) =
        varying.into_iter().partition(|&l| !looks_float(&fs[l]));
    p!("\n== varying lanes that are NOT plausible f32 (ints / bitfields / ids) ==");
    for &l in &intish {
        let mut s = bits[l].clone();
        s.sort_unstable();
        s.dedup();
        p!(
            "  {:#06x}: {} distinct u32, 240SX={} ; first few {:?}",
            l * 4,
            s.len(),
            bits[l][sx],
            &s[..s.len().min(8)]
        );
    }
    p!("  -> {} float-shaped varying lanes", varying.len());

    // ---------- block-level duplicates ----------
    p!("\n== byte-range duplicates (bit-identical in all 46 records) ==");
    // greedy: longest 4-aligned run starting at each offset that equals a run elsewhere
    let same_lane = |a: usize, b: usize| bits[a] == bits[b];
    let mut reported: Vec<(usize, usize, usize)> = Vec::new();
    for a in 0..NL {
        if kind[a] == "zero" || kind[a] == "string" {
            continue;
        }
        for b in (a + 1)..NL {
            if !same_lane(a, b) || kind[b] == "zero" {
                continue;
            }
            // extend
            let mut len = 1;
            while a + len < b && b + len < NL && same_lane(a + len, b + len) && kind[a + len] != "zero"
            {
                len += 1;
            }
            if len >= 2 && !reported.iter().any(|&(x, y, l)| a >= x && a < x + l && b == y + (a - x))
            {
                reported.push((a, b, len));
                p!(
                    "  {:#06x}..{:#06x}  ==  {:#06x}..{:#06x}   ({} lanes)",
                    a * 4,
                    (a + len) * 4,
                    b * 4,
                    (b + len) * 4,
                    len
                );
            }
            break;
        }
    }

    // ---------- 1. identical vectors ----------
    let mut byvec: BTreeMap<Vec<u32>, Vec<usize>> = BTreeMap::new();
    for &l in &varying {
        byvec.entry(bits[l].clone()).or_default().push(l);
    }
    let mut groups: Vec<Vec<usize>> = byvec.into_values().collect();
    groups.sort_by_key(|g| g[0]);
    p!("\n== 1. bit-identical 46-vectors ==");
    p!("  {} float-shaped varying lanes -> {} distinct vectors", varying.len(), groups.len());
    for g in groups.iter().filter(|g| g.len() > 1) {
        p!("  x{} : {}   [240SX = {:.6}]", g.len(), fmt_offs(g), fs[g[0]][sx]);
    }

    // ---------- 2+3. ratio against EVERY other lane in the record ----------
    p!("\n== 2+3. exact scalar multiples (tested against every lane in the record) ==");
    let reps: Vec<usize> = groups.iter().map(|g| g[0]).collect();
    let mut base: Vec<usize> = Vec::new();
    let mut nder = 0;
    for &l in &reps {
        // prefer an already-decoded / earlier lane as the base
        let mut hit = None;
        for b in 0..NL {
            if b == l || kind[b] == "zero" || kind[b] == "string" || kind[b] == "const" {
                continue;
            }
            if !looks_float(&fs[b]) {
                continue;
            }
            // only accept a base that is decoded, or an earlier independent lane
            let usable = lane_decoded(b).is_some() || base.contains(&b);
            if !usable {
                continue;
            }
            if let Some(k) = ratio(&fs[l], &fs[b]) {
                hit = Some((b, k));
                break;
            }
        }
        match hit {
            Some((b, k)) => {
                p!(
                    "  {:#06x} = {:.6} × {:#06x} {}",
                    l * 4,
                    k,
                    b * 4,
                    lane_decoded(b).unwrap_or("")
                );
                nder += 1;
            }
            None => base.push(l),
        }
    }
    p!("  -> {nder} derived, {} INDEPENDENT UNKNOWN QUANTITIES", base.len());

    // ---------- 4. partitions ----------
    p!("\n== 4. lanes inducing the SAME partition of the 46 cars ==");
    let mut bypart: BTreeMap<Vec<usize>, Vec<usize>> = BTreeMap::new();
    for &l in &base {
        let mut map: BTreeMap<u32, usize> = BTreeMap::new();
        let mut part = Vec::with_capacity(N);
        for &b in &bits[l] {
            let next = map.len();
            part.push(*map.entry(b).or_insert(next));
        }
        bypart.entry(part).or_default().push(l);
    }
    let mut parts: Vec<(usize, Vec<usize>)> =
        bypart.into_iter().map(|(k, v)| (k.iter().max().map_or(0, |m| m + 1), v)).collect();
    parts.sort_by_key(|(c, v)| (*c, v[0]));
    for (nclass, ls) in &parts {
        if ls.len() > 1 {
            p!("  {nclass:>2} classes, {} lanes: {}", ls.len(), fmt_offs(ls));
        }
    }
    p!("  distinct partitions among the {} unknowns: {}", base.len(), parts.len());

    // ---------- 5. cardinality table with per-lane detail ----------
    p!("\n== 5. every independent unknown, by cardinality ==");
    let mut card: Vec<(usize, usize)> = base
        .iter()
        .map(|&l| {
            let mut s = bits[l].clone();
            s.sort_unstable();
            s.dedup();
            (l, s.len())
        })
        .collect();
    card.sort_by_key(|&(_, c)| c);
    for &(l, c) in &card {
        let copies = groups.iter().find(|g| g[0] == l).map(|g| g.len()).unwrap_or(1);
        p!(
            "  {:#06x}  card {c:>2}  copies {copies}  240SX {:>10.5}  min {:>10.4}  max {:>10.4}",
            l * 4,
            fs[l][sx],
            fs[l].iter().cloned().fold(f32::MAX, f32::min),
            fs[l].iter().cloned().fold(f32::MIN, f32::max)
        );
    }

    p!("\n== 5b. value tables for card <= 5 ==");
    for &(l, c) in card.iter().filter(|&&(_, c)| c <= 5) {
        let mut per: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
        for (i, &b) in bits[l].iter().enumerate() {
            per.entry(b).or_default().push(&d.names[i]);
        }
        p!("  {:#06x} ({c} distinct):", l * 4);
        for (v, cars) in per {
            p!("      {:>12.5} × {:>2}: {}", f32::from_bits(v), cars.len(), cars.join(","));
        }
    }

    p!("\n== independent unknown lanes ==");
    p!("  {}", fmt_offs(&base));

    // ---------- 6. the 0x530 family vs the torque curve, checked per car ----------
    p!("\n== 6. is 0x0530[i] == torque[i]/4 ? ==");
    let mut worst = 0.0f32;
    let mut bad = 0;
    for c in 0..N {
        for i in 0..9 {
            let t = f32_at(&d.recs[c], 0x310 + i * 4);
            let b = f32_at(&d.recs[c], 0x530 + i * 4);
            let e = (b - t / 4.0).abs();
            if e > 1e-6 * t.abs().max(1e-6) {
                bad += 1;
                if e > worst {
                    worst = e;
                }
            }
        }
    }
    p!("  {bad} of 414 (46×9) points deviate; worst abs error {worst:.6}");
    for c in [sx, car(&d, "SKYLINE"), car(&d, "BUS")] {
        let t: Vec<f32> = (0..9).map(|i| f32_at(&d.recs[c], 0x310 + i * 4)).collect();
        let b: Vec<f32> = (0..12).map(|i| f32_at(&d.recs[c], 0x530 + i * 4)).collect();
        p!("  {:>10}: torque {:?}", d.names[c], t);
        p!("  {:>10}: 0x530  {:?}", "", b);
        p!("  {:>10}: ratio  {:?}", "", (0..9).map(|i| b[i] / t[i]).collect::<Vec<_>>());
    }

    // ---------- 7. the four 0x40 blocks at 0x520/0x560/0x5a0/0x5e0 ----------
    p!("\n== 7. the 0x520/0x560/0x5a0/0x5e0 block family, 240SX ==");
    for base_off in [0x520usize, 0x560, 0x5a0, 0x5e0] {
        let v: Vec<f32> = (0..16).map(|i| f32_at(&d.recs[sx], base_off + i * 4)).collect();
        p!("  {base_off:#06x}: {v:?}");
    }
    p!("\n== 7b. the gearbox-shaped blocks 0x2c0/0x460/0x4a0/0x4e0, 240SX ==");
    for base_off in [0x2C0usize, 0x460, 0x4A0, 0x4E0] {
        let v: Vec<f32> = (0..16).map(|i| f32_at(&d.recs[sx], base_off + i * 4)).collect();
        p!("  {base_off:#06x}: {v:?}");
    }
    p!("\n== 7c. axle-shaped blocks 0x280 / 0x2a0, 240SX + SKYLINE + BUS ==");
    for c in [sx, car(&d, "SKYLINE"), car(&d, "BUS")] {
        for base_off in [0x280usize, 0x2a0] {
            let v: Vec<f32> = (0..8).map(|i| f32_at(&d.recs[c], base_off + i * 4)).collect();
            p!("  {:>10} {base_off:#06x}: {v:?}", d.names[c]);
        }
    }
    p!("\n== 7d. 0x6c0..0x720 (240SX / BUS) ==");
    for c in [sx, car(&d, "BUS")] {
        let v: Vec<f32> = (0..24).map(|i| f32_at(&d.recs[c], 0x6c0 + i * 4)).collect();
        p!("  {:>10}: {v:?}", d.names[c]);
    }
    p!("\n== 7e. 0x620..0x660 and 0x330..0x3a0 (240SX) ==");
    let v: Vec<f32> = (0..16).map(|i| f32_at(&d.recs[sx], 0x620 + i * 4)).collect();
    p!("  0x620: {v:?}");
    let v: Vec<f32> = (0..28).map(|i| f32_at(&d.recs[sx], 0x330 + i * 4)).collect();
    p!("  0x330: {v:?}");
    let v: Vec<f32> = (0..12).map(|i| f32_at(&d.recs[sx], 0x3a0 + i * 4)).collect();
    p!("  0x3a0: {v:?}");
    let v: Vec<f32> = (0..16).map(|i| f32_at(&d.recs[sx], 0x420 + i * 4)).collect();
    p!("  0x420: {v:?}");
    let v: Vec<f32> = (0..12).map(|i| f32_at(&d.recs[sx], 0x100 + i * 4)).collect();
    p!("  0x100: {v:?}");
    let v: Vec<f32> = (0..12).map(|i| f32_at(&d.recs[sx], 0x1e0 + i * 4)).collect();
    p!("  0x1e0: {v:?}");
    let v: Vec<f32> = (0..12).map(|i| f32_at(&d.recs[sx], 0x260 + i * 4)).collect();
    p!("  0x260: {v:?}");
    let v: Vec<f32> = (0..20).map(|i| f32_at(&d.recs[sx], 0x740 + i * 4)).collect();
    p!("  0x740: {v:?}");
    let v: Vec<f32> = (0..20).map(|i| f32_at(&d.recs[sx], 0x7c0 + i * 4)).collect();
    p!("  0x7c0: {v:?}");
    let v: Vec<f32> = (0..20).map(|i| f32_at(&d.recs[sx], 0x840 + i * 4)).collect();
    p!("  0x840: {v:?}");

    std::fs::write("/tmp/lens2_7f3a91.txt", &o).unwrap();
    println!("wrote /tmp/lens2_7f3a91.txt ({} bytes)", o.len());
}

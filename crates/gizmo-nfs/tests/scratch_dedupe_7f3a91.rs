//! Throwaway: how many DISTINCT quantities live in the unread lanes of 0x00034600?

use std::collections::BTreeMap;
use std::fmt::Write as _;

const REC: usize = 0x890;
const NL: usize = REC / 4; // 548 lanes
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

fn find_chunk(b: &[u8], want: u32) -> Option<(usize, usize)> {
    let mut o = 0usize;
    while o + 8 <= b.len() {
        if u32_at(b, o) == want {
            let sz = u32_at(b, o + 4) as usize;
            if o + 8 + sz <= b.len() && sz == 8 + N * REC {
                return Some((o + 8, sz));
            }
        }
        o += 4;
    }
    None
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
    let (payload, _sz) = find_chunk(&bytes, 0x0003_4600).expect("chunk 0x00034600");
    let start = payload + 8;
    let mut recs = Vec::new();
    let mut names = Vec::new();
    for i in 0..N {
        let r = bytes[start + i * REC..start + (i + 1) * REC].to_vec();
        names.push(cstr(&r, 0, 0x10));
        recs.push(r);
    }
    Data { names, recs }
}

// ---------- decoded coverage ----------

fn decoded_ranges() -> Vec<(usize, usize, &'static str)> {
    let mut v: Vec<(usize, usize, &'static str)> = vec![
        (0x00, 0x10, "name"),
        (0x20, 0x30, "name2"),
        (0x40, 0x70, "path"),
        (0xC0, 0xD0, "manufacturer"),
        (0x220, 0x224, "mass"),
        (0x224, 0x230, "body LWH"),
        (0x300, 0x30C, "rpm"),
        (0x310, 0x334, "torque9"),
    ];
    for i in 0..4 {
        let b = 0x120 + i * 0x30;
        v.push((b + 0x00, b + 0x04, "wheel fore_aft"));
        v.push((b + 0x08, b + 0x0C, "wheel ride_h"));
        v.push((b + 0x10, b + 0x14, "wheel radius"));
        v.push((b + 0x14, b + 0x18, "wheel tyre_w"));
        v.push((b + 0x1C, b + 0x20, "wheel lateral"));
    }
    for &g in &[0x2C0usize, 0x460, 0x4A0, 0x4E0] {
        v.push((g + 0x08, g + 0x0C, "final_drive"));
        v.push((g + 0x10, g + 0x14, "rear_drive"));
        v.push((g + 0x18, g + 0x1C, "gear count"));
        v.push((g + 0x20, g + 0x40, "gear ratios"));
    }
    v
}

fn lane_decoded(lane: usize) -> Option<&'static str> {
    let o = lane * 4;
    for (a, b, n) in decoded_ranges() {
        if o < b && o + 4 > a {
            return Some(n);
        }
    }
    None
}

/// a == k * b for one k across all cars, with k meaningfully non-zero.
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

/// a == k*b + c, least squares, judged by max abs residual relative to a's spread.
fn affine(a: &[f32], b: &[f32]) -> Option<(f32, f32)> {
    let n = a.len() as f64;
    let (sx, sy): (f64, f64) = b.iter().zip(a).fold((0.0, 0.0), |s, (x, y)| {
        (s.0 + *x as f64, s.1 + *y as f64)
    });
    let (mx, my) = (sx / n, sy / n);
    let mut sxx = 0.0f64;
    let mut sxy = 0.0f64;
    for i in 0..a.len() {
        let dx = b[i] as f64 - mx;
        sxx += dx * dx;
        sxy += dx * (a[i] as f64 - my);
    }
    if sxx < 1e-12 {
        return None;
    }
    let k = sxy / sxx;
    let c = my - k * mx;
    if k.abs() < 1e-9 {
        return None;
    }
    let spread = {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for &y in a {
            lo = lo.min(y as f64);
            hi = hi.max(y as f64);
        }
        (hi - lo).max(1e-9)
    };
    let mut worst = 0.0f64;
    for i in 0..a.len() {
        worst = worst.max((a[i] as f64 - (k * b[i] as f64 + c)).abs());
    }
    if worst / spread > 1e-4 {
        return None;
    }
    Some((k as f32, c as f32))
}

fn is_effectively_zero(v: &[f32]) -> bool {
    v.iter().all(|x| x.abs() < 1e-30)
}

fn looks_float(v: &[f32]) -> bool {
    // every value finite and either 0 or in a sane magnitude band
    v.iter().all(|x| x.is_finite() && (*x == 0.0 || (x.abs() > 1e-8 && x.abs() < 1e9)))
}

fn known_quantities(d: &Data) -> Vec<(String, Vec<f32>)> {
    let g = |o: usize| -> Vec<f32> { d.recs.iter().map(|r| f32_at(r, o)).collect() };
    let mut v: Vec<(String, Vec<f32>)> = Vec::new();
    v.push(("mass".into(), g(0x220)));
    v.push(("body_L".into(), g(0x224)));
    v.push(("body_W".into(), g(0x228)));
    v.push(("body_H".into(), g(0x22C)));
    v.push(("rpm_idle".into(), g(0x300)));
    v.push(("rpm_red".into(), g(0x304)));
    v.push(("rpm_lim".into(), g(0x308)));
    for i in 0..9 {
        v.push((format!("torque[{i}]"), g(0x310 + i * 4)));
    }
    for (bi, &b) in [0x2C0usize, 0x460, 0x4A0, 0x4E0].iter().enumerate() {
        v.push((format!("final_drive[{bi}]"), g(b + 0x08)));
        v.push((format!("rear_drive[{bi}]"), g(b + 0x10)));
        for j in 0..8 {
            v.push((format!("gear[{bi}][{j}]"), g(b + 0x20 + j * 4)));
        }
    }
    for i in 0..4 {
        let b = 0x120 + i * 0x30;
        v.push((format!("wheel{i}_foreaft"), g(b)));
        v.push((format!("wheel{i}_ride"), g(b + 0x08)));
        v.push((format!("wheel{i}_radius"), g(b + 0x10)));
        v.push((format!("wheel{i}_tyrew"), g(b + 0x14)));
        v.push((format!("wheel{i}_lateral"), g(b + 0x1C)));
    }
    let fa0 = g(0x120);
    let fa2 = g(0x120 + 2 * 0x30);
    v.push(("wheelbase".into(), fa0.iter().zip(&fa2).map(|(a, b)| a - b).collect()));
    let la0 = g(0x120 + 0x1C);
    let la1 = g(0x120 + 0x30 + 0x1C);
    v.push(("track_front".into(), la0.iter().zip(&la1).map(|(a, b)| a - b).collect()));
    let m = g(0x220);
    let l = g(0x224);
    let w = g(0x228);
    let h = g(0x22C);
    v.push(("inertia_x".into(), (0..N).map(|i| m[i] / 12.0 * (w[i] * w[i] + h[i] * h[i])).collect()));
    v.push(("inertia_y".into(), (0..N).map(|i| m[i] / 12.0 * (l[i] * l[i] + h[i] * h[i])).collect()));
    v.push(("inertia_z".into(), (0..N).map(|i| m[i] / 12.0 * (l[i] * l[i] + w[i] * w[i])).collect()));
    v.push(("L*W".into(), (0..N).map(|i| l[i] * w[i]).collect()));
    v.push(("W*H".into(), (0..N).map(|i| w[i] * h[i]).collect()));
    v.push(("L*H".into(), (0..N).map(|i| l[i] * h[i]).collect()));
    v.push(("L2".into(), (0..N).map(|i| l[i] * l[i]).collect()));
    v.push(("W2".into(), (0..N).map(|i| w[i] * w[i]).collect()));
    v.push(("H2".into(), (0..N).map(|i| h[i] * h[i]).collect()));
    v.push(("1/mass".into(), (0..N).map(|i| 1.0 / m[i]).collect()));
    v
}

#[test]
fn lens() {
    let d = load();
    let mut o = String::new();
    macro_rules! p { ($($t:tt)*) => { { writeln!(o, $($t)*).unwrap(); } } }

    p!("cars ({}):", d.names.len());
    for (i, n) in d.names.iter().enumerate() {
        p!("  [{i:2}] {n}");
    }

    // ---- string map: which byte ranges are ASCII text in every record ----
    p!("\n== byte ranges that are text in all 46 records ==");
    let mut textish = vec![false; REC];
    for off in 0..REC {
        let all = d.recs.iter().all(|r| {
            let c = r[off];
            c == 0 || (0x20..0x7f).contains(&c)
        });
        let any_letter = d.recs.iter().any(|r| r[off].is_ascii_alphanumeric());
        textish[off] = all && any_letter;
    }
    let mut i = 0;
    while i < REC {
        if textish[i] {
            let s = i;
            while i < REC && textish[i] {
                i += 1;
            }
            p!("  {s:#06x}..{i:#06x}  e.g. 240SX: {:?}", cstr(&d.recs[car(&d, "240SX")], s, i - s));
        } else {
            i += 1;
        }
    }

    // lane matrices
    let mut bits: Vec<Vec<u32>> = Vec::with_capacity(NL);
    let mut fs: Vec<Vec<f32>> = Vec::with_capacity(NL);
    for l in 0..NL {
        let off = l * 4;
        bits.push(d.recs.iter().map(|r| u32_at(r, off)).collect());
        fs.push(d.recs.iter().map(|r| f32_at(r, off)).collect());
    }

    let mut zero = Vec::new();
    let mut constant = Vec::new();
    let mut varying = Vec::new();
    let mut decoded = Vec::new();
    let mut text_lane = Vec::new();
    for l in 0..NL {
        if lane_decoded(l).is_some() {
            decoded.push(l);
            continue;
        }
        if (0..4).any(|k| textish[l * 4 + k]) {
            text_lane.push(l);
            continue;
        }
        let b = &bits[l];
        if b.iter().all(|&x| x == 0) {
            zero.push(l);
        } else if b.iter().all(|&x| x == b[0]) {
            constant.push(l);
        } else {
            varying.push(l);
        }
    }
    p!(
        "\n== coverage (548 lanes) ==\ndecoded {} | text {} | zero {} | constant {} | VARYING {}",
        decoded.len(),
        text_lane.len(),
        zero.len(),
        constant.len(),
        varying.len()
    );
    p!("  text lanes: {}", fmt_offs(&text_lane));

    // near-zero varying (sign bits / denormals) — not real quantities
    let (nearzero, varying): (Vec<usize>, Vec<usize>) =
        varying.into_iter().partition(|&l| is_effectively_zero(&fs[l]));
    p!("  varying-but-effectively-zero-as-f32 ({}): {}", nearzero.len(), fmt_offs(&nearzero));
    for &l in &nearzero {
        let mut s: Vec<u32> = bits[l].clone();
        s.sort_unstable();
        s.dedup();
        p!("      {:#06x}: {} distinct raw values, e.g. {:?}", l * 4, s.len(), &s[..s.len().min(6)]);
    }
    p!("  -> real varying lanes: {}", varying.len());

    // ---- 1. identical vectors ----
    let mut byvec: BTreeMap<Vec<u32>, Vec<usize>> = BTreeMap::new();
    for &l in &varying {
        byvec.entry(bits[l].clone()).or_default().push(l);
    }
    let mut groups: Vec<Vec<usize>> = byvec.into_values().collect();
    groups.sort_by_key(|g| g[0]);
    p!("\n== 1. bit-identical 46-vectors ==");
    p!("  {} varying lanes -> {} distinct vectors", varying.len(), groups.len());
    let sx = car(&d, "240SX");
    for g in groups.iter().filter(|g| g.len() > 1) {
        let l = g[0];
        p!(
            "  x{} : {}   [240SX = {} / {:.6}]",
            g.len(),
            fmt_offs(g),
            bits[l][sx],
            fs[l][sx]
        );
    }

    let reps: Vec<usize> = groups.iter().map(|g| g[0]).collect();

    // ---- 2. scalar multiples between unread lanes ----
    p!("\n== 2. exact scalar multiples between unread lanes ==");
    let mut base: Vec<usize> = Vec::new();
    let mut nder = 0;
    for &l in &reps {
        if !looks_float(&fs[l]) {
            base.push(l);
            continue;
        }
        let mut found = None;
        for &b in &base {
            if !looks_float(&fs[b]) {
                continue;
            }
            if let Some(k) = ratio(&fs[l], &fs[b]) {
                found = Some((b, k));
                break;
            }
        }
        match found {
            Some((b, k)) => {
                p!("  {:#06x} = {:.6} × {:#06x}", l * 4, k, b * 4);
                nder += 1;
            }
            None => base.push(l),
        }
    }
    p!("  -> {nder} derived, {} left", base.len());

    // ---- 3. multiples of decoded quantities ----
    p!("\n== 3. exact multiples of already-decoded quantities ==");
    let known = known_quantities(&d);
    let mut still: Vec<usize> = Vec::new();
    let mut n3 = 0;
    for &l in &base {
        if !looks_float(&fs[l]) {
            still.push(l);
            continue;
        }
        let mut hit = None;
        for (name, v) in &known {
            if let Some(k) = ratio(&fs[l], v) {
                hit = Some((name.clone(), k));
                break;
            }
        }
        match hit {
            Some((n, k)) => {
                p!("  {:#06x} = {:.6} × {}", l * 4, k, n);
                n3 += 1;
            }
            None => still.push(l),
        }
    }
    p!("  -> {n3} derived, {} still unexplained by ratio", still.len());

    // ---- 3b. affine of decoded, and affine between unknowns ----
    p!("\n== 3b. affine (k·x + c) relations, |resid| < 1e-4 of spread ==");
    let mut still2: Vec<usize> = Vec::new();
    let mut n3b = 0;
    for &l in &still {
        if !looks_float(&fs[l]) {
            still2.push(l);
            continue;
        }
        let mut hit = None;
        for (name, v) in &known {
            if let Some((k, c)) = affine(&fs[l], v) {
                hit = Some((name.clone(), k, c));
                break;
            }
        }
        if hit.is_none() {
            for &b in &still2 {
                if !looks_float(&fs[b]) {
                    continue;
                }
                if let Some((k, c)) = affine(&fs[l], &fs[b]) {
                    hit = Some((format!("lane {:#06x}", b * 4), k, c));
                    break;
                }
            }
        }
        match hit {
            Some((n, k, c)) => {
                p!("  {:#06x} = {:.6} × {} + {:.6}", l * 4, k, n, c);
                n3b += 1;
            }
            None => still2.push(l),
        }
    }
    p!("  -> {n3b} affine-derived, {} INDEPENDENT UNKNOWNS", still2.len());

    // ---- 4. the answer ----
    p!("\n== 4. ANSWER ==");
    p!("  {} unread varying lanes", varying.len() + nearzero.len());
    p!("  {} distinct value-vectors (after dedup)", groups.len());
    p!("  {} independent unknown quantities after removing derived", still2.len());

    // ---- 5. cardinality ----
    p!("\n== 5. cardinality of every independent unknown ==");
    let mut card: Vec<(usize, usize)> = still2
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
        let dupes = groups.iter().find(|g| g[0] == l).map(|g| g.len()).unwrap_or(1);
        p!(
            "  {:#06x}  {c:>2} distinct  (x{dupes} copies)  240SX={:.5}  range {:.4}..{:.4}",
            l * 4,
            fs[l][sx],
            fs[l].iter().cloned().fold(f32::MAX, f32::min),
            fs[l].iter().cloned().fold(f32::MIN, f32::max)
        );
    }

    p!("\n== 5b. full value table for the <=4-distinct lanes ==");
    for &(l, c) in card.iter().filter(|&&(_, c)| c <= 4) {
        let mut per: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
        for (i, &b) in bits[l].iter().enumerate() {
            per.entry(b).or_default().push(&d.names[i]);
        }
        p!("  {:#06x} ({c} distinct):", l * 4);
        for (v, cars) in per {
            p!("      f32 {:>12.5} × {:>2}: {}", f32::from_bits(v), cars.len(), cars.join(","));
        }
    }

    p!("\n== independent unknown lane list ==");
    p!("  {}", fmt_offs(&still2));

    // ---- 6. constant lanes, for completeness (they are known-but-uniform) ----
    p!("\n== constant-across-46 lanes (one value each; not per-car quantities) ==");
    let mut byval: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for &l in &constant {
        byval.entry(bits[l][0]).or_default().push(l);
    }
    for (v, ls) in byval {
        p!("  f32 {:>14.5} (u32 {v}) at {}", f32::from_bits(v), fmt_offs(&ls));
    }

    std::fs::write("/tmp/lens_7f3a91.txt", &o).unwrap();
    println!("{o}");
}

fn fmt_offs(v: &[usize]) -> String {
    v.iter().map(|&x| format!("{:#06x}", x * 4)).collect::<Vec<_>>().join(" ")
}

fn car(d: &Data, n: &str) -> usize {
    d.names.iter().position(|x| x == n).unwrap_or(0)
}

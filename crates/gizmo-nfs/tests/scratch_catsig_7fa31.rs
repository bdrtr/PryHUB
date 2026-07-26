//! Throwaway: categorical-signature lens over GLOBALB 0x00034600. DELETE ME.
#![allow(clippy::needless_range_loop, dead_code)]

use gizmo_nfs::chunk::ChunkNode;
use std::collections::{BTreeMap, BTreeSet};

const REC: usize = 0x890;
const CHUNK: u32 = 0x0003_4600;
const NLANE: usize = REC / 4; // 548

fn load() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("NFSU2_ROOT")?).join("GLOBAL/GLOBALB.BUN");
    std::fs::read(p).ok()
}

fn cstr(b: &[u8], off: usize, max: usize) -> String {
    let s = &b[off..(off + max).min(b.len())];
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

struct Db {
    names: Vec<String>,
    recs: Vec<Vec<u8>>,
    /// u[lane][car]
    u: Vec<[u32; 46]>,
    f: Vec<[f32; 46]>,
}

fn build() -> Option<Db> {
    let bytes = load()?;
    let nodes = ChunkNode::parse(&bytes).ok()?;
    let mut hit: Option<Vec<u8>> = None;
    fn walk(n: &ChunkNode, root: &[u8], out: &mut Option<Vec<u8>>) {
        if n.header.id == CHUNK {
            *out = Some(n.data(root).to_vec());
        }
        for c in &n.children {
            walk(c, root, out);
        }
    }
    for n in &nodes {
        walk(n, &bytes, &mut hit);
    }
    let d = hit?;
    let mut names = Vec::new();
    let mut recs = Vec::new();
    for i in 0..46 {
        let r = d[8 + i * REC..8 + (i + 1) * REC].to_vec();
        names.push(cstr(&r, 0, 0x10));
        recs.push(r);
    }
    let mut u = vec![[0u32; 46]; NLANE];
    let mut f = vec![[0f32; 46]; NLANE];
    for l in 0..NLANE {
        for c in 0..46 {
            let o = l * 4;
            let v = u32::from_le_bytes([recs[c][o], recs[c][o + 1], recs[c][o + 2], recs[c][o + 3]]);
            u[l][c] = v;
            f[l][c] = f32::from_bits(v);
        }
    }
    Some(Db { names, recs, u, f })
}

// ---------------------------------------------------------------- decoded map

/// Every 4-byte lane the committed parser (or its proved-but-unread docs) already reads.
fn decoded() -> BTreeMap<usize, &'static str> {
    let mut m = BTreeMap::new();
    let mut put = |o: usize, n: usize, s: &'static str| {
        for k in 0..n {
            m.insert(o + k * 4, s);
        }
    };
    put(0x00, 4, "name");
    put(0x10, 4, "STRINGPAD?"); // between name and name2 - unknown, kept separate below
    put(0x20, 4, "name2");
    put(0x40, 12, "path");
    put(0xC0, 4, "manufacturer");
    for w in 0..4 {
        let b = 0x120 + w * 0x30;
        put(b, 1, "wheel.fore_aft");
        put(b + 0x08, 1, "wheel.ride_height");
        put(b + 0x10, 1, "wheel.radius");
        put(b + 0x14, 1, "wheel.tyre_width");
        put(b + 0x1C, 1, "wheel.lateral");
    }
    put(0x220, 1, "mass");
    put(0x224, 3, "body_lwh");
    put(0x230, 16, "inertia4x4(proved,unread)");
    for &g in &[0x2C0usize, 0x460, 0x4A0, 0x4E0] {
        put(g + 0x08, 1, "gearbox.final_drive");
        put(g + 0x10, 1, "gearbox.rear_drive");
        put(g + 0x18, 1, "gearbox.count");
        put(g + 0x20, 8, "gearbox.ratios");
    }
    put(0x300, 3, "rpm");
    put(0x310, 9, "torque");
    m.remove(&0x10);
    m.remove(&0x14);
    m.remove(&0x18);
    m.remove(&0x1C);
    m
}

fn is_str_region(o: usize) -> bool {
    (0x00..0x40).contains(&o) || (0x40..0xC0).contains(&o) || (0xC0..0xD0).contains(&o)
}

fn fmt_f(v: f32) -> String {
    if v == 0.0 {
        "0".into()
    } else if v.is_finite() && v.abs() < 1e7 && v.abs() > 1e-6 {
        format!("{v:.4}")
    } else {
        format!("bits{:08X}", v.to_bits())
    }
}

fn set_str(s: &BTreeSet<usize>, names: &[String]) -> String {
    s.iter().map(|&i| names[i].as_str()).collect::<Vec<_>>().join(",")
}

#[test]
fn categorical_signatures() {
    let Some(db) = build() else {
        eprintln!("SKIP: no NFSU2_ROOT");
        return;
    };
    let n = &db.names;

    // ---------------------------------------------------------------- 1. split
    // Derive the traffic/playable split from arithmetic, not a list.
    let redline: Vec<f32> = (0..46).map(|c| db.f[0x304 / 4][c]).collect();
    let limiter: Vec<f32> = (0..46).map(|c| db.f[0x308 / 4][c]).collect();
    let mut traffic = BTreeSet::new();
    let mut playable = BTreeSet::new();
    for c in 0..46 {
        if (limiter[c] - redline[c] - 1000.0).abs() < 0.5 {
            traffic.insert(c);
        } else if (limiter[c] - redline[c] - 500.0).abs() < 0.5 {
            playable.insert(c);
        }
    }
    println!("== SPLIT from (limiter-redline) ==");
    println!("playable {}: {}", playable.len(), set_str(&playable, n));
    println!("traffic  {}: {}", traffic.len(), set_str(&traffic, n));
    assert_eq!(playable.len() + traffic.len(), 46);

    // rear_drive partition
    let rd = &db.f[0x2D0 / 4];
    let mut rd_groups: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
    for c in 0..46 {
        rd_groups.entry(rd[c].to_bits()).or_default().insert(c);
    }
    println!("\n== rear_drive groups ==");
    for (k, v) in &rd_groups {
        println!("  {:>8} ({:2}) {}", fmt_f(f32::from_bits(*k)), v.len(), set_str(v, n));
    }

    // ---------------------------------------------------------------- 2. lane census
    let dec = decoded();
    let mut kind: Vec<&str> = vec![""; NLANE];
    let mut zero_all = Vec::new();
    let mut constant = Vec::new();
    let mut varying = Vec::new();
    for l in 0..NLANE {
        let o = l * 4;
        if dec.contains_key(&o) {
            kind[l] = "decoded";
            continue;
        }
        if is_str_region(o) {
            kind[l] = "string-region";
            continue;
        }
        let v = &db.u[l];
        if v.iter().all(|&x| x == 0) {
            kind[l] = "zero";
            zero_all.push(l);
        } else if v.iter().all(|&x| x == v[0]) {
            kind[l] = "const";
            constant.push(l);
        } else {
            kind[l] = "vary";
            varying.push(l);
        }
    }
    println!(
        "\n== census == total {NLANE} lanes; decoded {} string {} zero {} const {} VARY {}",
        kind.iter().filter(|k| **k == "decoded").count(),
        kind.iter().filter(|k| **k == "string-region").count(),
        zero_all.len(),
        constant.len(),
        varying.len()
    );

    // ---------------------------------------------------------------- 3. equivalence classes
    // Exact byte-equal 46-vectors collapse to one quantity.
    let mut classes: BTreeMap<Vec<u32>, Vec<usize>> = BTreeMap::new();
    for &l in &varying {
        classes.entry(db.u[l].to_vec()).or_default().push(l);
    }
    let dup: Vec<&Vec<usize>> = classes.values().filter(|v| v.len() > 1).collect();
    let n_dup_lanes: usize = dup.iter().map(|v| v.len()).sum();
    println!(
        "\n== exact-duplicate classes among {} varying lanes: {} classes covering {} lanes -> saves {} ==",
        varying.len(),
        dup.len(),
        n_dup_lanes,
        n_dup_lanes - dup.len()
    );
    let mut dupsorted: Vec<&Vec<usize>> = dup.clone();
    dupsorted.sort_by_key(|v| v[0]);
    for v in &dupsorted {
        println!(
            "  [{}]  e.g. 240SX={}",
            v.iter().map(|l| format!("+0x{:03X}", l * 4)).collect::<Vec<_>>().join(" "),
            fmt_f(db.f[v[0]][3])
        );
    }

    // also: varying lanes exactly equal to a DECODED lane
    println!("\n== varying unread lanes byte-equal to a decoded lane ==");
    for &l in &varying {
        for (&o, name) in &dec {
            if is_str_region(o) {
                continue;
            }
            if db.u[l] == db.u[o / 4] {
                println!("  +0x{:03X} == +0x{:03X} ({name})", l * 4, o);
            }
        }
    }

    // ---------------------------------------------------------------- 4. proportional / affine to a reference
    // References: decoded scalars + one representative per duplicate class + all singleton lanes.
    let mut reps: Vec<usize> = classes.values().map(|v| v[0]).collect();
    reps.sort_unstable();
    let mut dec_lanes: Vec<usize> =
        dec.keys().filter(|o| !is_str_region(**o)).map(|o| o / 4).collect();
    dec_lanes.sort_unstable();
    dec_lanes.dedup();

    // scale-only relation B = k*A across all 46, k != 0,1
    println!("\n== scale relations (B = k*A over all 46 cars) ==");
    let all_ref: Vec<usize> = dec_lanes.iter().chain(reps.iter()).copied().collect();
    let mut explained: BTreeSet<usize> = BTreeSet::new();
    for &b in &reps {
        for &a in &all_ref {
            if a == b || db.u[a] == db.u[b] {
                continue;
            }
            // need a valid k
            let mut k = f32::NAN;
            let mut ok = true;
            for c in 0..46 {
                let (x, y) = (db.f[a][c] as f64, db.f[b][c] as f64);
                if !x.is_finite() || !y.is_finite() {
                    ok = false;
                    break;
                }
                if x.abs() < 1e-9 {
                    if y.abs() > 1e-9 {
                        ok = false;
                        break;
                    }
                    continue;
                }
                let kk = (y / x) as f32;
                if k.is_nan() {
                    k = kk;
                } else if (kk - k).abs() > 1e-4 * k.abs().max(1e-3) {
                    ok = false;
                    break;
                }
            }
            if ok && k.is_finite() && k != 0.0 {
                let label = dec.get(&(a * 4)).copied().unwrap_or("unread");
                println!("  +0x{:03X} = {:.6} * +0x{:03X}  ({label})", b * 4, k, a * 4);
                explained.insert(b);
                break;
            }
        }
    }

    // ---------------------------------------------------------------- 5. categorical signatures
    println!("\n== per-lane categorical signature (varying, unread, class representatives) ==");
    println!("off    #dist  zeros                     signature");
    let mut sig_traffic_zero = Vec::new();
    let mut sig_playable_zero = Vec::new();
    let mut sig_const_on_playable = Vec::new();
    let mut small_groups: Vec<(usize, BTreeMap<u32, BTreeSet<usize>>)> = Vec::new();
    let mut matches_rd = Vec::new();
    let mut refines_rd = Vec::new();
    for &l in &reps {
        let mut groups: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
        for c in 0..46 {
            groups.entry(db.u[l][c]).or_default().insert(c);
        }
        let zeros: BTreeSet<usize> = (0..46).filter(|&c| db.u[l][c] == 0).collect();
        let ndist = groups.len();
        let mut sig = String::new();
        if !zeros.is_empty() {
            if zeros == traffic {
                sig.push_str("ZERO=traffic ");
                sig_traffic_zero.push(l);
            } else if zeros == playable {
                sig.push_str("ZERO=playable ");
                sig_playable_zero.push(l);
            }
        }
        // constant across playable, differs on traffic
        let pv: BTreeSet<u32> = playable.iter().map(|&c| db.u[l][c]).collect();
        let tv: BTreeSet<u32> = traffic.iter().map(|&c| db.u[l][c]).collect();
        if pv.len() == 1 && tv.len() > 1 {
            sig.push_str("CONST-on-playable ");
            sig_const_on_playable.push(l);
        }
        if tv.len() == 1 && pv.len() > 1 {
            sig.push_str("CONST-on-traffic ");
        }
        // partition vs rear_drive
        let part: BTreeSet<BTreeSet<usize>> = groups.values().cloned().collect();
        let rdpart: BTreeSet<BTreeSet<usize>> = rd_groups.values().cloned().collect();
        if part == rdpart {
            sig.push_str("PARTITION=rear_drive ");
            matches_rd.push(l);
        } else if part.iter().all(|g| rdpart.iter().any(|r| g.is_subset(r))) && ndist > 1 {
            sig.push_str("refines(rear_drive) ");
            refines_rd.push(l);
        }
        if ndist <= 6 {
            small_groups.push((l, groups.clone()));
        }
        println!(
            "+0x{:03X}  {:3}   {:2}  {}",
            l * 4,
            ndist,
            zeros.len(),
            sig
        );
    }

    println!("\n== lanes whose zero-set is EXACTLY the traffic set ==");
    for l in &sig_traffic_zero {
        println!("  +0x{:03X}  playable values: {}", l * 4, sample(&db, *l, &playable));
    }
    println!("== lanes whose zero-set is EXACTLY the playable set ==");
    for l in &sig_playable_zero {
        println!("  +0x{:03X}  traffic values: {}", l * 4, sample(&db, *l, &traffic));
    }
    println!("== lanes constant over playable, varying over traffic ==");
    for l in &sig_const_on_playable {
        println!(
            "  +0x{:03X}  playable={}  traffic groups:",
            l * 4,
            fmt_f(db.f[*l][3])
        );
        let mut g: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
        for &c in &traffic {
            g.entry(db.u[*l][c]).or_default().insert(c);
        }
        for (k, v) in &g {
            println!("       {:>10} {}", fmt_f(f32::from_bits(*k)), set_str(v, n));
        }
    }

    println!("\n== lanes with <=6 distinct values, with members ==");
    for (l, g) in &small_groups {
        println!("  +0x{:03X}:", l * 4);
        for (k, v) in g {
            println!("     {:>12} ({:2}) {}", fmt_f(f32::from_bits(*k)), v.len(), set_str(v, n));
        }
    }

    println!("\n== lanes matching the rear_drive partition exactly: {:?} ==",
        matches_rd.iter().map(|l| format!("+0x{:03X}", l * 4)).collect::<Vec<_>>());
    println!("== lanes refining the rear_drive partition: {:?} ==",
        refines_rd.iter().map(|l| format!("+0x{:03X}", l * 4)).collect::<Vec<_>>());
}

fn sample(db: &Db, l: usize, set: &BTreeSet<usize>) -> String {
    let vals: BTreeSet<u32> = set.iter().map(|&c| db.u[l][c]).collect();
    let mut s: Vec<String> = vals.iter().take(8).map(|&b| fmt_f(f32::from_bits(b))).collect();
    if vals.len() > 8 {
        s.push(format!("... ({} distinct)", vals.len()));
    }
    s.join(" ")
}

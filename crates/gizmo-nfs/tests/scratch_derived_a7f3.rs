//! THROWAWAY. Lens: which unread lanes of 0x00034600 are functions of what the crate knows.

use std::collections::BTreeMap;
use std::path::PathBuf;

const REC: usize = 0x890;
const NL: usize = REC / 4; // 548 four-byte lanes

fn root() -> Option<PathBuf> {
    std::env::var_os("NFSU2_ROOT").map(PathBuf::from)
}

fn le_f32(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn le_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn cstr(b: &[u8], off: usize, max: usize) -> String {
    let s = &b[off..(off + max).min(b.len())];
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}
fn is_car_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 13
        && s.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

/// Same scan the parser uses, but returning the record's file offset.
fn record_offsets(b: &[u8]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut seen_end = 0usize;
    let mut i = 0usize;
    while i + 0x40 < b.len() {
        let Some(rel) = b[i..].windows(4).position(|w| w == b"CARS") else { break };
        let path_pos = i + rel;
        i = path_pos + 1;
        if path_pos < 0x40 {
            continue;
        }
        let rec = path_pos - 0x40;
        if rec < seen_end || rec + REC > b.len() {
            continue;
        }
        let name = cstr(b, rec, 0x10);
        if !is_car_name(&name) || cstr(b, rec + 0x20, 0x10) != name {
            continue;
        }
        if !cstr(b, path_pos, 0x30).contains("GEOMETRY") {
            continue;
        }
        seen_end = rec + REC;
        out.push((name, rec));
    }
    out
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..x.len() {
        let a = x[i] - mx;
        let c = y[i] - my;
        sxy += a * c;
        sxx += a * a;
        syy += c * c;
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return 0.0;
    }
    (sxy / (sxx.sqrt() * syy.sqrt())).clamp(-1.0, 1.0)
}

/// Least-squares y = a*x + b, plus the worst residual as a fraction of y's spread.
fn fit(x: &[f64], y: &[f64]) -> (f64, f64, f64) {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for i in 0..x.len() {
        sxy += (x[i] - mx) * (y[i] - my);
        sxx += (x[i] - mx) * (x[i] - mx);
    }
    let a = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    let b = my - a * mx;
    let ymin = y.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = y.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = (ymax - ymin).max(1e-30);
    let mut worst: f64 = 0.0;
    for i in 0..x.len() {
        worst = worst.max((y[i] - (a * x[i] + b)).abs() / span);
    }
    (a, b, worst)
}

fn floatish(v: f32) -> bool {
    v == 0.0 || (v.is_finite() && v.abs() >= 1e-6 && v.abs() <= 1e7)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Float,
    Int,
    Text,
}

#[test]
fn lens_derived_lanes() {
    let Some(root) = root() else {
        eprintln!("NFSU2_ROOT unset");
        return;
    };
    let bytes = std::fs::read(root.join("GLOBAL/GLOBALB.BUN")).expect("read GLOBALB.BUN");
    let offs = record_offsets(&bytes);
    let cars = gizmo_nfs::globalb::parse_cartypeinfos(&bytes);
    assert_eq!(offs.len(), 46);
    assert_eq!(cars.len(), 46);
    for (i, c) in cars.iter().enumerate() {
        assert_eq!(c.name, offs[i].0, "scan order must match the parser's");
    }
    let n = 46usize;
    let recs: Vec<&[u8]> = offs.iter().map(|(_, o)| &bytes[*o..*o + REC]).collect();

    // ---- decoded lane mask -------------------------------------------------
    let mut decoded = [false; NL];
    let mut mark = |from: usize, to: usize, d: &mut [bool; NL]| {
        let mut o = from;
        while o < to {
            d[o / 4] = true;
            o += 4;
        }
    };
    mark(0x00, 0x10, &mut decoded); // name
    mark(0x20, 0x30, &mut decoded); // name again
    mark(0x40, 0x70, &mut decoded); // CARS\..\GEOMETRY.BIN path
    mark(0xC0, 0xD0, &mut decoded); // manufacturer
    for i in 0..4 {
        let b = 0x120 + i * 0x30;
        for f in [0x00, 0x08, 0x10, 0x14, 0x1C] {
            decoded[(b + f) / 4] = true;
        }
    }
    decoded[0x220 / 4] = true; // mass
    mark(0x224, 0x230, &mut decoded); // body L,W,H
    mark(0x300, 0x30C, &mut decoded); // rpm triple
    mark(0x310, 0x334, &mut decoded); // 9 torque points
    for g in [0x2C0usize, 0x460, 0x4A0, 0x4E0] {
        for f in [0x08, 0x10, 0x18] {
            decoded[(g + f) / 4] = true;
        }
        mark(g + 0x20, g + 0x40, &mut decoded); // 8 ratio slots
    }
    let n_decoded = decoded.iter().filter(|d| **d).count();

    // ---- classify every lane ----------------------------------------------
    let mut kind = [Kind::Int; NL];
    let mut allzero = [false; NL];
    let mut constant = [false; NL];
    for l in 0..NL {
        let o = l * 4;
        let words: Vec<u32> = recs.iter().map(|r| le_u32(r, o)).collect();
        allzero[l] = words.iter().all(|w| *w == 0);
        constant[l] = words.iter().all(|w| *w == words[0]) && !allzero[l];
        let texty = recs.iter().all(|r| {
            (0..4).all(|k| {
                let c = r[o + k];
                c == 0 || (0x20..0x7f).contains(&c)
            })
        }) && recs.iter().any(|r| (0..4).any(|k| r[o + k] >= 0x41));
        kind[l] = if texty {
            Kind::Text
        } else if recs.iter().all(|r| floatish(le_f32(r, o))) {
            Kind::Float
        } else {
            Kind::Int
        };
    }

    // ---- predictors --------------------------------------------------------
    let mut pred: Vec<(String, Vec<f64>)> = Vec::new();
    let mut push = |name: &str, v: Vec<f64>, p: &mut Vec<(String, Vec<f64>)>| {
        // only keep predictors that actually vary
        if v.iter().any(|x| (*x - v[0]).abs() > 1e-12) {
            p.push((name.to_string(), v));
        }
    };
    let g = |f: &dyn Fn(&gizmo_nfs::globalb::CarTypeInfo) -> f64| -> Vec<f64> {
        cars.iter().map(|c| f(c)).collect()
    };
    push("mass_kg", g(&|c| c.mass_kg as f64), &mut pred);
    push("wheelbase", g(&|c| (c.wheels[0].fore_aft - c.wheels[2].fore_aft) as f64), &mut pred);
    push("track_front", g(&|c| (c.wheels[0].lateral - c.wheels[1].lateral) as f64), &mut pred);
    push("track_rear", g(&|c| (c.wheels[3].lateral - c.wheels[2].lateral) as f64), &mut pred);
    for i in 0..4 {
        push(&format!("radius[{i}]"), g(&|c| c.wheels[i].radius as f64), &mut pred);
        push(&format!("tyre_w[{i}]"), g(&|c| c.handling.tyre_width_m[i] as f64), &mut pred);
        push(&format!("fore_aft[{i}]"), g(&|c| c.wheels[i].fore_aft as f64), &mut pred);
        push(&format!("lateral[{i}]"), g(&|c| c.wheels[i].lateral as f64), &mut pred);
        push(&format!("ride_h[{i}]"), g(&|c| c.wheels[i].ride_height as f64), &mut pred);
    }
    push("body_L", g(&|c| c.handling.body_m[0] as f64), &mut pred);
    push("body_W", g(&|c| c.handling.body_m[1] as f64), &mut pred);
    push("body_H", g(&|c| c.handling.body_m[2] as f64), &mut pred);
    push("rear_drive", g(&|c| c.handling.rear_drive as f64), &mut pred);
    push("idle_rpm", g(&|c| c.handling.engine.idle_rpm as f64), &mut pred);
    push("redline_rpm", g(&|c| c.handling.engine.red_line_rpm as f64), &mut pred);
    push("limiter_rpm", g(&|c| c.handling.engine.limiter_rpm as f64), &mut pred);
    for i in 0..9 {
        push(&format!("torque[{i}]"), g(&|c| c.handling.torque_nm[i] as f64), &mut pred);
    }
    push(
        "torque_peak",
        g(&|c| c.handling.torque_nm.iter().cloned().fold(f32::MIN, f32::max) as f64),
        &mut pred,
    );
    push(
        "torque_sum",
        g(&|c| c.handling.torque_nm.iter().sum::<f32>() as f64),
        &mut pred,
    );
    for k in 0..4 {
        push(&format!("final_drive[{k}]"), g(&|c| c.handling.gearbox[k].final_drive as f64), &mut pred);
        push(&format!("gear_count[{k}]"), g(&|c| c.handling.gearbox[k].count as f64), &mut pred);
        push(&format!("gear1[{k}]"), g(&|c| c.handling.gearbox[k].forward[0] as f64), &mut pred);
        push(&format!("reverse[{k}]"), g(&|c| c.handling.gearbox[k].reverse as f64), &mut pred);
    }
    // derived physical combinations
    let m = |c: &gizmo_nfs::globalb::CarTypeInfo| c.mass_kg as f64 / 1000.0; // tonnes, as stored
    let wb = |c: &gizmo_nfs::globalb::CarTypeInfo| (c.wheels[0].fore_aft - c.wheels[2].fore_aft) as f64;
    let bl = |c: &gizmo_nfs::globalb::CarTypeInfo| c.handling.body_m[0] as f64;
    let bw = |c: &gizmo_nfs::globalb::CarTypeInfo| c.handling.body_m[1] as f64;
    let bh = |c: &gizmo_nfs::globalb::CarTypeInfo| c.handling.body_m[2] as f64;
    push("m*wheelbase", g(&|c| m(c) * wb(c)), &mut pred);
    push("m/wheelbase", g(&|c| m(c) / wb(c)), &mut pred);
    push("m*L", g(&|c| m(c) * bl(c)), &mut pred);
    push("m*W", g(&|c| m(c) * bw(c)), &mut pred);
    push("m*H", g(&|c| m(c) * bh(c)), &mut pred);
    push("m/12*(W2+H2)", g(&|c| m(c) / 12.0 * (bw(c) * bw(c) + bh(c) * bh(c))), &mut pred);
    push("m/12*(L2+H2)", g(&|c| m(c) / 12.0 * (bl(c) * bl(c) + bh(c) * bh(c))), &mut pred);
    push("m/12*(L2+W2)", g(&|c| m(c) / 12.0 * (bl(c) * bl(c) + bw(c) * bw(c))), &mut pred);
    push("m*r0^2", g(&|c| m(c) * (c.wheels[0].radius as f64).powi(2)), &mut pred);
    push("L*W", g(&|c| bl(c) * bw(c)), &mut pred);
    push("W*H", g(&|c| bw(c) * bh(c)), &mut pred);
    push("L*W*H", g(&|c| bl(c) * bw(c) * bh(c)), &mut pred);
    push(
        "peak*finaldrive0",
        g(&|c| {
            c.handling.torque_nm.iter().cloned().fold(f32::MIN, f32::max) as f64
                * c.handling.gearbox[0].final_drive as f64
        }),
        &mut pred,
    );
    push(
        "peak*fd0*g1/r0",
        g(&|c| {
            c.handling.torque_nm.iter().cloned().fold(f32::MIN, f32::max) as f64
                * c.handling.gearbox[0].final_drive as f64
                * c.handling.gearbox[0].forward[0] as f64
                / c.wheels[0].radius as f64
        }),
        &mut pred,
    );
    push(
        "peak/mass",
        g(&|c| c.handling.torque_nm.iter().cloned().fold(f32::MIN, f32::max) as f64 / m(c)),
        &mut pred,
    );
    // the traffic / playable split, as a predictor in its own right
    let playable: Vec<bool> = cars
        .iter()
        .map(|c| (c.handling.engine.limiter_rpm - c.handling.engine.red_line_rpm - 500.0).abs() < 0.01)
        .collect();
    push(
        "IS_PLAYABLE(binary)",
        playable.iter().map(|p| if *p { 1.0 } else { 0.0 }).collect(),
        &mut pred,
    );

    let play_idx: Vec<usize> = (0..n).filter(|i| playable[*i]).collect();

    println!("== records {n}, lanes {NL}, decoded lanes {n_decoded}");
    println!(
        "== playable {} / traffic {}",
        play_idx.len(),
        n - play_idx.len()
    );
    println!("== predictors {}", pred.len());

    // ---- the unread, varying lanes ----------------------------------------
    let mut unread: Vec<usize> = Vec::new();
    for l in 0..NL {
        if !decoded[l] && !allzero[l] && !constant[l] && kind[l] != Kind::Text {
            unread.push(l);
        }
    }
    let unread_text: Vec<usize> =
        (0..NL).filter(|l| !decoded[*l] && !allzero[*l] && !constant[*l] && kind[*l] == Kind::Text).collect();
    println!(
        "== unread varying lanes {} (+{} text-looking)",
        unread.len(),
        unread_text.len()
    );
    if !unread_text.is_empty() {
        println!(
            "   text-looking unread: {:?}",
            unread_text.iter().map(|l| format!("+0x{:03x}", l * 4)).collect::<Vec<_>>()
        );
    }

    // numeric value of each unread lane, per its kind
    let val = |l: usize| -> Vec<f64> {
        recs.iter()
            .map(|r| match kind[l] {
                Kind::Float => le_f32(r, l * 4) as f64,
                _ => le_u32(r, l * 4) as f64,
            })
            .collect()
    };

    // ---- group by exact bitwise identity across all 46 ----------------------
    let mut groups: BTreeMap<Vec<u32>, Vec<usize>> = BTreeMap::new();
    for &l in &unread {
        let key: Vec<u32> = recs.iter().map(|r| le_u32(r, l * 4)).collect();
        groups.entry(key).or_default().push(l);
    }
    let mut reps: Vec<Vec<usize>> = groups.values().cloned().collect();
    reps.sort_by_key(|g| g[0]);
    println!("== distinct 46-vectors among unread varying lanes: {}", reps.len());
    println!("-- exact duplicate families (>1 lane) --");
    for gset in &reps {
        if gset.len() > 1 {
            println!(
                "   {:2} lanes identical: {}",
                gset.len(),
                gset.iter().map(|l| format!("+0x{:03x}", l * 4)).collect::<Vec<_>>().join(" ")
            );
        }
    }

    // ---- correlate each representative against every predictor -------------
    struct Row {
        lanes: Vec<usize>,
        kind: Kind,
        distinct: usize,
        best: Vec<(String, f64, f64, f64, f64)>, // name, r, slope, intercept, worst-resid-frac
        r_play_of_best: f64,
        sample: (f64, f64, f64),
    }
    let mut rows: Vec<Row> = Vec::new();
    let name_of = |nm: &str| cars.iter().position(|c| c.name == nm).unwrap();
    let i240 = name_of("240SX");
    let ibus = name_of("BUS");
    let icel = name_of("CELICA");

    for gset in &reps {
        let l = gset[0];
        let y = val(l);
        let mut scored: Vec<(String, f64, f64, f64, f64)> = Vec::new();
        for (pn, px) in &pred {
            let r = pearson(px, &y);
            let (a, b, w) = fit(px, &y);
            scored.push((pn.clone(), r, a, b, w));
        }
        scored.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        let bestname = scored[0].0.clone();
        let bp = pred.iter().find(|(n, _)| *n == bestname).unwrap();
        let xs: Vec<f64> = play_idx.iter().map(|i| bp.1[*i]).collect();
        let ys: Vec<f64> = play_idx.iter().map(|i| y[*i]).collect();
        let r_play = pearson(&xs, &ys);
        let mut distinct: Vec<u64> = y.iter().map(|v| v.to_bits()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        rows.push(Row {
            lanes: gset.clone(),
            kind: kind[l],
            distinct: distinct.len(),
            best: scored.into_iter().take(3).collect(),
            r_play_of_best: r_play,
            sample: (y[i240], y[ibus], y[icel]),
        });
    }

    // ---- (1) lanes explained: |r| > 0.9 -----------------------------------
    println!("\n######## (1) |r| > 0.9 against a decoded quantity ########");
    println!("lane(s)                     kind dst  r      slope        intercept    maxresid  r|playable  best predictor   [240SX/BUS/CELICA]");
    let mut explained = 0usize;
    for row in rows.iter().filter(|r| r.best[0].1.abs() > 0.9) {
        explained += row.lanes.len();
        let (pn, r, a, b, w) = &row.best[0];
        println!(
            "{:26} {:5} {:3} {:+.4} {:+.6e} {:+.6e} {:.2e}  {:+.4}  {:<16} [{:.5}/{:.5}/{:.5}]",
            row.lanes.iter().map(|l| format!("+0x{:03x}", l * 4)).collect::<Vec<_>>().join(","),
            match row.kind { Kind::Float => "f32", Kind::Int => "u32", Kind::Text => "txt" },
            row.distinct, r, a, b, w, row.r_play_of_best, pn,
            row.sample.0, row.sample.1, row.sample.2
        );
    }
    println!("   -> {explained} of {} unread varying lanes have an |r|>0.9 partner", unread.len());

    // ---- exact functional matches (residual ~ 0) ---------------------------
    println!("\n######## (1b) EXACT affine functions (max residual < 1e-4 of span) ########");
    for row in &rows {
        for (pn, r, a, b, w) in &row.best {
            if *w < 1e-4 && r.abs() > 0.999 {
                println!(
                    "{:26} = {:+.8e} * {} {:+.6e}   (maxresid {:.1e} of span)",
                    row.lanes.iter().map(|l| format!("+0x{:03x}", l * 4)).collect::<Vec<_>>().join(","),
                    a, pn, b, w
                );
                break;
            }
        }
    }

    // ---- (2) lanes correlating with nothing --------------------------------
    println!("\n######## (2) max |r| < 0.5 — independent of everything decoded ########");
    println!("lane(s)                     kind dst  best|r| predictor         [240SX/BUS/CELICA]");
    let mut indep: Vec<&Row> = rows.iter().filter(|r| r.best[0].1.abs() < 0.5).collect();
    indep.sort_by_key(|r| r.lanes[0]);
    for row in &indep {
        println!(
            "{:26} {:5} {:3} {:.4}  {:<18} [{:.5}/{:.5}/{:.5}]",
            row.lanes.iter().map(|l| format!("+0x{:03x}", l * 4)).collect::<Vec<_>>().join(","),
            match row.kind { Kind::Float => "f32", Kind::Int => "u32", Kind::Text => "txt" },
            row.distinct,
            row.best[0].1.abs(),
            row.best[0].0,
            row.sample.0, row.sample.1, row.sample.2
        );
    }
    println!(
        "   -> {} lanes in {} vectors",
        indep.iter().map(|r| r.lanes.len()).sum::<usize>(),
        indep.len()
    );

    // ---- middle band -------------------------------------------------------
    println!("\n######## (2b) 0.5 <= max|r| <= 0.9 ########");
    for row in rows.iter().filter(|r| (0.5..=0.9).contains(&r.best[0].1.abs())) {
        println!(
            "{:26} {:5} {:3} r={:+.4} ({}) r|play={:+.4} [{:.5}/{:.5}/{:.5}]",
            row.lanes.iter().map(|l| format!("+0x{:03x}", l * 4)).collect::<Vec<_>>().join(","),
            match row.kind { Kind::Float => "f32", Kind::Int => "u32", Kind::Text => "txt" },
            row.distinct,
            row.best[0].1, row.best[0].0, row.r_play_of_best,
            row.sample.0, row.sample.1, row.sample.2
        );
    }

    // ---- (4) cross-correlate the unexplained lanes with each other ----------
    println!("\n######## (3) clusters among unread lanes themselves (|r| >= 0.999) ########");
    let unexplained: Vec<&Row> = rows.iter().filter(|r| r.best[0].1.abs() <= 0.9).collect();
    let vecs: Vec<Vec<f64>> = unexplained.iter().map(|r| val(r.lanes[0])).collect();
    let k = unexplained.len();
    let mut parent: Vec<usize> = (0..k).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for a in 0..k {
        for b in (a + 1)..k {
            if pearson(&vecs[a], &vecs[b]).abs() >= 0.999 {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for a in 0..k {
        let r = find(&mut parent, a);
        clusters.entry(r).or_default().push(a);
    }
    let mut multi = 0usize;
    for (_, members) in clusters.iter() {
        if members.len() > 1 {
            multi += 1;
            let all: Vec<String> = members
                .iter()
                .flat_map(|m| unexplained[*m].lanes.iter().map(|l| format!("+0x{:03x}", l * 4)))
                .collect();
            // report the pairwise relation to the first member
            let base = &vecs[members[0]];
            let rel: Vec<String> = members[1..]
                .iter()
                .map(|m| {
                    let (a, b, w) = fit(base, &vecs[*m]);
                    format!("{:+.4}x{:+.4e}(res {:.0e})", a, b, w)
                })
                .collect();
            println!("   cluster: {}   rel-to-first: {}", all.join(" "), rel.join(" "));
        }
    }
    println!("   -> {} multi-lane clusters among {} unexplained vectors", multi, k);
    println!(
        "   -> distinct unknown quantities after clustering: {}",
        clusters.len()
    );

    // ---- (4) same-value-shape families: lanes whose 46-vector is a scalar multiple
    println!("\n######## (3b) unread lane == constant * another unread lane, exactly ########");
    for a in 0..k {
        for b in (a + 1)..k {
            let (x, y) = (&vecs[a], &vecs[b]);
            // ratio test through the origin
            let mut ratios: Vec<f64> = Vec::new();
            let mut ok = true;
            for i in 0..n {
                if x[i].abs() < 1e-12 {
                    if y[i].abs() > 1e-12 {
                        ok = false;
                        break;
                    }
                } else {
                    ratios.push(y[i] / x[i]);
                }
            }
            if !ok || ratios.len() < 20 {
                continue;
            }
            let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
            let dev = ratios.iter().map(|r| (r - mean).abs() / mean.abs().max(1e-12)).fold(0.0, f64::max);
            if dev < 1e-5 {
                println!(
                    "   +0x{:03x} * {:.6} == +0x{:03x}  (max rel dev {:.1e})",
                    unexplained[a].lanes[0] * 4,
                    mean,
                    unexplained[b].lanes[0] * 4,
                    dev
                );
            }
        }
    }

    // ---- explicit structural checks the brief asks about --------------------
    println!("\n######## structural probes ########");
    // the two 0x0280 / 0x02a0 blocks
    for (tag, base) in [("A@0x280", 0x280usize), ("B@0x2a0", 0x2a0)] {
        let mut s = String::new();
        for j in 0..8 {
            s.push_str(&format!("{:+.5} ", le_f32(recs[i240], base + j * 4)));
        }
        println!("   {tag} 240SX: {s}");
    }
    // are the two blocks equal per car?
    let mut same_ab = 0;
    for r in &recs {
        if (0..7).all(|j| le_u32(r, 0x280 + j * 4) == le_u32(r, 0x2a0 + j * 4)) {
            same_ab += 1;
        }
    }
    println!("   0x280 block == 0x2a0 block bitwise in {same_ab}/46 cars");

    // the two nine-runs at 0x3e0 / 0x404 against the torque curve
    for base in [0x3e0usize, 0x404] {
        let mut worst_rel: f64 = 0.0;
        let mut ratios = Vec::new();
        for (ci, r) in recs.iter().enumerate() {
            for j in 0..9 {
                let v = le_f32(r, base + j * 4) as f64;
                let t = cars[ci].handling.torque_nm[j] as f64 / 1000.0;
                if t.abs() > 1e-9 {
                    ratios.push(v / t);
                }
            }
        }
        let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
        for r in &ratios {
            worst_rel = worst_rel.max((r - mean).abs() / mean.abs().max(1e-12));
        }
        println!(
            "   run@0x{base:03x} / torque: mean ratio {:.6}, worst rel dev {:.2e}  (240SX first three {:.5} {:.5} {:.5})",
            mean, worst_rel,
            le_f32(recs[i240], base), le_f32(recs[i240], base + 4), le_f32(recs[i240], base + 8)
        );
    }
    // the four 12-lane tables
    for base in [0x530usize, 0x570, 0x5b0, 0x5f0] {
        let mut ratios = Vec::new();
        for (ci, r) in recs.iter().enumerate() {
            for j in 0..9 {
                let v = le_f32(r, base + j * 4) as f64;
                let t = cars[ci].handling.torque_nm[j] as f64 / 1000.0;
                if t.abs() > 1e-9 {
                    ratios.push(v / t);
                }
            }
        }
        let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
        let worst = ratios.iter().map(|r| (r - mean).abs() / mean.abs()).fold(0.0, f64::max);
        let tail: Vec<String> =
            (9..12).map(|j| format!("{:.5}", le_f32(recs[i240], base + j * 4))).collect();
        println!(
            "   table@0x{base:03x}: 9 values / torque -> mean {:.6}, worst rel dev {:.2e}; lanes 9..12 (240SX) {}",
            mean, worst, tail.join(" ")
        );
    }
    // 0x290 across cars
    println!("   +0x290 per car:");
    let mut line = String::new();
    for (i, c) in cars.iter().enumerate() {
        line.push_str(&format!("{}={:.3} ", c.name, le_f32(recs[i], 0x290)));
        if i % 5 == 4 {
            println!("      {line}");
            line.clear();
        }
    }
    if !line.is_empty() {
        println!("      {line}");
    }
}

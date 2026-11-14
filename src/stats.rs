//! 中央値・分位点・書き出しの数の形。

/// 並べた値の線形補間の分位点（Excel の PERCENTILE.INC と同じ）。`sorted` は昇順。
pub fn percentile_sorted(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let h = (sorted.len() - 1) as f64 * p.clamp(0.0, 1.0);
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    Some(sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo]))
}

/// 昇順に並べた写しを返す。NaN は除く。
pub fn sorted(values: &[f64]) -> Vec<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// 値の列の要約。
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub median: f64,
    pub max: f64,
    pub p5: f64,
    pub p95: f64,
}

pub fn summarize(values: &[f64]) -> Option<Summary> {
    let s = sorted(values);
    if s.is_empty() {
        return None;
    }
    Some(Summary {
        n: s.len(),
        min: s[0],
        median: percentile_sorted(&s, 0.5)?,
        max: s[s.len() - 1],
        p5: percentile_sorted(&s, 0.05)?,
        p95: percentile_sorted(&s, 0.95)?,
    })
}

/// 小数 6 桁までの、指数の表記を使わない文字列。末尾の 0 は落とす。
pub fn fmt6(v: f64) -> String {
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// 値を小数 6 桁で丸める（JSON に書くとき）。
pub fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn stats_median_odd_even_single() {
        assert!(close(summarize(&[0.1, 0.5, 0.2]).unwrap().median, 0.2));
        assert!(close(summarize(&[0.2, 0.4]).unwrap().median, 0.3));
        assert!(close(summarize(&[0.7]).unwrap().median, 0.7));
        assert!(summarize(&[]).is_none());
    }

    #[test]
    fn stats_percentile_inc() {
        // Excel: PERCENTILE.INC({1,2,3,4}, 0.05) = 1.15, 0.95 = 3.85
        let s = summarize(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        assert!(close(s.p5, 1.15), "{}", s.p5);
        assert!(close(s.p95, 3.85), "{}", s.p95);
        assert!(close(s.median, 2.5));
        assert_eq!((s.min, s.max, s.n), (1.0, 4.0, 4));
    }

    #[test]
    fn stats_fmt6_has_no_exponent() {
        assert_eq!(fmt6(0.000002), "0.000002");
        assert_eq!(fmt6(0.78425), "0.78425");
        assert_eq!(fmt6(1.0), "1");
        assert_eq!(fmt6(0.0), "0");
        assert_eq!(fmt6(-0.0000001), "0");
        assert_eq!(fmt6(0.1000000015), "0.1");
    }
}

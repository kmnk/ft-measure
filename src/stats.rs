//! 書き出しの数の形。

/// 値を小数 6 桁で丸める（JSON に書くとき）。
pub fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

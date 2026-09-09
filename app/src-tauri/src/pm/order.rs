// 看板列内排序键：base36 小数的 fractional indexing。
// key 视为 0.key 的 36 进制小数，比较用普通字符串序（去尾零的规范形下与数值序一致）。

const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const BASE: i64 = 36;

/// 首条 item 的初始 key（字母表正中）
pub const INITIAL_KEY: &str = "g";

fn digit_val(c: u8) -> Option<i64> {
    DIGITS.iter().position(|&d| d == c).map(|p| p as i64)
}

fn val_digit(v: i64) -> char {
    DIGITS[v as usize] as char
}

fn parse_key(key: &str) -> Result<Vec<i64>, String> {
    key.bytes()
        .map(|b| digit_val(b).ok_or_else(|| format!("非法排序键字符: {}", b as char)))
        .collect()
}

/// lo < hi 前提下求严格中点。hi_int=1 表示上界为 1.0（即「最后一名之后」）。
fn midpoint(lo: &[i64], hi_int: i64, hi: &[i64]) -> Result<String, String> {
    let n = lo.len().max(hi.len());
    // 分数部分相加（右对齐进位）
    let mut sum = vec![0i64; n + 1];
    let mut carry = 0i64;
    for i in (0..n).rev() {
        let s = lo.get(i).copied().unwrap_or(0) + hi.get(i).copied().unwrap_or(0) + carry;
        sum[i + 1] = s % BASE;
        carry = s / BASE;
    }
    sum[0] = carry + hi_int; // 整数位
    // 整体除 2（左到右传余数；余数最多再产一位 18='i' 即除尽，因为 2·36ⁿ | 36ⁿ⁺¹·18）
    let mut out: Vec<i64> = Vec::with_capacity(n + 2);
    let mut rem = 0i64;
    for d in sum {
        let v = rem * BASE + d;
        out.push(v / 2);
        rem = v % 2;
    }
    while rem > 0 {
        let v = rem * BASE;
        out.push(v / 2);
        rem = v % 2;
    }
    // out[0] 是整数位（必为 0），其后是小数位；去尾零得规范形
    let mut frac = &out[1..];
    while let Some((&0, rest)) = frac.split_last() {
        frac = rest;
    }
    if frac.is_empty() {
        return Err("排序键中点计算失败（上下界相邻？）".into());
    }
    Ok(frac.iter().map(|&v| val_digit(v)).collect())
}

/// 计算 between before 与 after 的新排序键；None 分别表示列首之前 / 列尾之后
pub fn key_between(before: Option<&str>, after: Option<&str>) -> Result<String, String> {
    match (before, after) {
        (None, None) => Ok(INITIAL_KEY.into()),
        (Some(a), Some(b)) => {
            if a >= b {
                return Err(format!("排序键区间无效: {a:?} 必须小于 {b:?}"));
            }
            midpoint(&parse_key(a)?, 0, &parse_key(b)?)
        }
        (Some(a), None) => midpoint(&parse_key(a)?, 1, &[]),
        (None, Some(b)) => midpoint(&[], 0, &parse_key(b)?),
    }
}

pub fn is_valid_key(key: &str) -> bool {
    !key.is_empty() && parse_key(key).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_between(lo: Option<&str>, mid: &str, hi: Option<&str>) {
        if let Some(a) = lo {
            assert!(a < mid, "{a:?} < {mid:?}");
        }
        if let Some(b) = hi {
            assert!(mid < b, "{mid:?} < {hi:?}");
        }
    }

    #[test]
    fn basic_midpoints() {
        assert_eq!(key_between(None, None).unwrap(), "g");
        let m = key_between(Some("g"), None).unwrap();
        assert_between(Some("g"), &m, None);
        let m = key_between(None, Some("g")).unwrap();
        assert_between(None, &m, Some("g"));
        let m = key_between(Some("a"), Some("b")).unwrap();
        assert_between(Some("a"), &m, Some("b"));
        // 相邻 key 也能继续细分
        let m = key_between(Some("a"), Some("a0i")).unwrap();
        assert_between(Some("a"), &m, Some("a0i"));
    }

    #[test]
    fn rejects_inverted_range() {
        assert!(key_between(Some("b"), Some("a")).is_err());
        assert!(key_between(Some("a"), Some("a")).is_err());
    }

    #[test]
    fn random_inserts_stay_ordered() {
        // 用 LCG 伪随机模拟 500 次插入，keys 排序必须与插入语义一致
        let mut keys: Vec<String> = Vec::new(); // 始终保持有序
        let mut seed: u64 = 0x9e3779b97f4a7c15;
        let mut rand = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..500 {
            let pos = rand() % (keys.len() + 1);
            let before = pos.checked_sub(1).map(|i| keys[i].as_str());
            let after = keys.get(pos).map(|s| s.as_str());
            let k = key_between(before, after).unwrap();
            keys.insert(pos, k);
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "排序键顺序与插入顺序不一致");
        }
    }

    #[test]
    fn repeated_head_insert() {
        // 持续往列首插，key 应不断变长但不溢出、不回绕
        let mut head: Option<String> = None;
        for _ in 0..200 {
            let k = key_between(None, head.as_deref()).unwrap();
            if let Some(h) = &head {
                assert!(k < *h);
            }
            head = Some(k);
        }
    }
}

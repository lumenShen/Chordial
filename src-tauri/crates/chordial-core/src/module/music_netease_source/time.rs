//! 时间换算工具：网易云时间戳 → 年份。

/// 将网易云的 publishTime（毫秒级 Unix 时间戳）转换为年份。
///
/// 返回 None 表示时间戳无效（<=0）。
pub fn publish_time_to_year(ms: i64) -> Option<u32> {
    if ms <= 0 {
        return None;
    }
    let secs = (ms / 1000) as u64;
    let mut days = secs / 86400;
    let mut year = 1970u32;
    loop {
        let days_in_year: u64 = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    Some(year)
}

/// 判断是否为闰年。
pub fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_year_from_millis() {
        // 1970-01-01 00:00:00 UTC = 0 ms
        assert_eq!(publish_time_to_year(0), None);
        // 2024-01-01 ≈ 1704067200000 ms
        assert_eq!(publish_time_to_year(1_704_067_200_000), Some(2024));
        // 1970-01-02 ≈ 86400000 ms
        assert_eq!(publish_time_to_year(86_400_000), Some(1970));
        // 1971-01-01 ≈ 31536000000 ms (365 days)
        assert_eq!(publish_time_to_year(31_536_000_000), Some(1971));
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000)); // 能被 400 整除
        assert!(is_leap_year(2024)); // 能被 4 整除但不被 100
        assert!(!is_leap_year(1900)); // 能被 100 整除但不被 400
        assert!(!is_leap_year(2023));
    }
}
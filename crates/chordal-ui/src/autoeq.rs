//! AutoEQ 耳机档案：从 jaakkopasanen/AutoEq 仓库拉取真实的参数均衡配置。
//!
//! 仓库的 `results/README.md` 是一份推荐结果索引（约 6000 款耳机），每行
//! 指向一个型号目录；目录里的 `<型号> ParametricEQ.txt` 是 AutoEq 生成的
//! 参数均衡文本。这里的策略：
//!
//! - 打开 AutoEQ 分类时拉取一次索引（约 500 KB），缓存到配置目录旁的
//!   `autoeq/index.md`，此后启动直接读缓存；
//! - 预览/应用某个型号时才拉取它的 ParametricEQ.txt（约 1 KB），缓存到
//!   `autoeq/params/`，同一型号只下载一次；
//! - 网络失败时保留已有列表与缓存，只把错误显示在界面上，不影响离线使用。
//!
//! 网络请求是阻塞的，调用方必须放在后台执行器里跑（与下载/封面一致）。

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use chordal_player::{EqBand, EqSettings, FilterKind, fmt_db};

/// 仓库 raw 文件的两个镜像。jsDelivr 走 CDN，通常比 GitHub 直连快；
/// GitHub raw 作为回退（网络环境不同，二者可用性会互换）。
const REPO_BASES: [&str; 2] = [
    "https://cdn.jsdelivr.net/gh/jaakkopasanen/AutoEq@master/results/",
    "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/",
];
const INDEX_FILE: &str = "README.md";
/// 索引超过一周时后台静默刷新，但继续展示缓存。
const INDEX_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// 单个镜像的超时：连不上时尽快切下一个镜像，避免首次打开长时间空等。
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoEqStatus {
    /// 还没打开过 AutoEQ 分类。
    #[default]
    Idle,
    /// 正在拉取索引；已有缓存时列表仍然可用。
    Loading,
    Ready,
    /// 索引拉取失败且没有缓存。
    Failed,
}

/// 索引里的一只耳机。曲线在预览/应用时按需拉取，因此 `bands` 是 `Option`。
#[derive(Debug, Clone)]
pub struct AutoEqEntry {
    pub name: String,
    /// 测量来源（仓库路径第一段），如 `oratory1990`。
    pub source: String,
    /// 头戴式 / 入耳式 / 耳塞式 / 其他。
    pub category: String,
    /// 仓库内相对路径（URL 编码），同时作为缓存键。
    pub file: String,
    /// 预计算的小写检索串（名称 + 来源）。
    search: String,
    pub bands: Option<Vec<EqBand>>,
    pub input_gain_db: f32,
    pub loading: bool,
    pub error: Option<String>,
}

impl AutoEqEntry {
    pub fn subtitle(&self) -> String {
        match &self.bands {
            Some(bands) => format!(
                "{} · {} · {} 段 · 前级 {}",
                self.source,
                self.category,
                bands.len(),
                fmt_db(self.input_gain_db)
            ),
            None => format!("{} · {}", self.source, self.category),
        }
    }

    /// 尚未拉取曲线时返回 `None`。
    pub fn to_settings(&self) -> Option<EqSettings> {
        let bands = self.bands.clone()?;
        Some(
            EqSettings {
                enabled: true,
                input_gain_db: self.input_gain_db,
                output_gain_db: 0.0,
                bands,
            }
            .sanitized(),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct AutoEqLibrary {
    /// 用 `Arc` 共享给虚拟列表的渲染闭包，避免每帧克隆 6000 条索引。
    pub entries: Arc<Vec<AutoEqEntry>>,
    pub status: AutoEqStatus,
    pub error: Option<String>,
    /// 当前列表是否来自本地缓存（用于界面提示）。
    pub from_cache: bool,
    pub index_loading: bool,
    /// 紧凑布局点按某个型号后，等曲线拉取完成再自动应用。
    pub pending_apply: Option<String>,
}

impl AutoEqLibrary {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn find(&self, file: &str) -> Option<&AutoEqEntry> {
        self.entries.iter().find(|entry| entry.file == file)
    }

    pub fn find_mut(&mut self, file: &str) -> Option<&mut AutoEqEntry> {
        Arc::make_mut(&mut self.entries)
            .iter_mut()
            .find(|entry| entry.file == file)
    }

    /// 索引刷新后合并新列表，保留已经拉取过的曲线、在途请求与错误状态，
    /// 避免预览或「拉取完自动应用」被索引同步打断。
    pub fn replace_entries(&mut self, mut entries: Vec<AutoEqEntry>) {
        type Carry = (Option<Vec<EqBand>>, f32, bool, Option<String>);
        let mut carried: HashMap<String, Carry> = self
            .entries
            .iter()
            .filter(|entry| entry.bands.is_some() || entry.loading || entry.error.is_some())
            .map(|entry| {
                (
                    entry.file.clone(),
                    (
                        entry.bands.clone(),
                        entry.input_gain_db,
                        entry.loading,
                        entry.error.clone(),
                    ),
                )
            })
            .collect();
        for entry in &mut entries {
            if let Some((bands, gain, loading, error)) = carried.remove(&entry.file) {
                entry.bands = bands;
                entry.input_gain_db = gain;
                entry.loading = loading;
                entry.error = error;
            }
        }
        self.entries = Arc::new(entries);
    }

    /// 分类 + 搜索过滤后的索引；返回下标而不是克隆条目。
    ///
    /// 搜索忽略空白：`HD650` 也能匹配 `Sennheiser HD 650`。
    pub fn visible_indices(&self, category: &str, query: &str) -> Vec<usize> {
        let query: String = query
            .trim()
            .to_lowercase()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| category == "全部" || entry.category == category)
            .filter(|(_, entry)| query.is_empty() || entry.search.contains(&query))
            .map(|(index, _)| index)
            .collect()
    }
}

/// AutoEQ 缓存目录：与 `config.json` 同级，便携模式（`CHORDIAL_CONFIG`）也会随之移动。
pub fn cache_dir() -> PathBuf {
    crate::config::config_path().with_file_name("autoeq")
}

fn index_cache_path(dir: &Path) -> PathBuf {
    dir.join("index.md")
}

fn param_cache_path(dir: &Path, file: &str) -> PathBuf {
    // 用稳定 FNV-1a 短名，避开路径分隔符与 Windows 文件名限制。
    dir.join("params")
        .join(format!("{:016x}.txt", fnv1a(file.as_bytes())))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 读取本地缓存的索引；返回列表与文件修改时间。
pub fn load_cached_index() -> Option<(Vec<AutoEqEntry>, SystemTime)> {
    load_cached_index_from(&cache_dir())
}

fn load_cached_index_from(dir: &Path) -> Option<(Vec<AutoEqEntry>, SystemTime)> {
    let path = index_cache_path(dir);
    let text = std::fs::read_to_string(&path).ok()?;
    let entries = parse_index(&text);
    if entries.is_empty() {
        return None;
    }
    let modified = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Some((entries, modified))
}

pub fn index_cache_is_stale(modified: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age > INDEX_MAX_AGE)
        .unwrap_or(true)
}

pub fn load_cached_param(file: &str) -> Option<(Vec<EqBand>, f32)> {
    let dir = cache_dir();
    let text = std::fs::read_to_string(param_cache_path(&dir, file)).ok()?;
    parse_param_eq(&text).ok()
}

/// 拉取索引并写入缓存。阻塞：调用方放在后台执行器里。
pub fn fetch_index_blocking() -> Result<Vec<AutoEqEntry>, String> {
    fetch_index_into(&cache_dir())
}

fn fetch_index_into(dir: &Path) -> Result<Vec<AutoEqEntry>, String> {
    let text = fetch_text(INDEX_FILE)?;
    let entries = parse_index(&text);
    if entries.is_empty() {
        return Err("AutoEq 索引为空或格式已变化".into());
    }
    write_atomic(&index_cache_path(dir), text.as_bytes());
    Ok(entries)
}

/// 拉取单个型号的 ParametricEQ.txt 并写入缓存。阻塞。
pub fn fetch_param_blocking(file: &str) -> Result<(Vec<EqBand>, f32), String> {
    fetch_param_into(&cache_dir(), file)
}

fn fetch_param_into(dir: &Path, file: &str) -> Result<(Vec<EqBand>, f32), String> {
    let text = fetch_text(file)?;
    let parsed = parse_param_eq(&text)?;
    write_atomic(&param_cache_path(dir, file), text.as_bytes());
    Ok(parsed)
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension("part");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// 进程级 HTTP 客户端：复用连接与 TLS 会话，单曲/单个型号的 JSON/文本
/// 请求不必每次重新握手（首次拉取 ~3s，后续通常 <1s）。
fn http_client() -> Result<&'static reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .connect_timeout(HTTP_CONNECT_TIMEOUT)
                .user_agent(concat!(
                    "Chordial/",
                    env!("CARGO_PKG_VERSION"),
                    " (AutoEQ results)"
                ))
                .build()
                .map_err(|error| format!("创建网络客户端失败：{error}"))
        })
        .as_ref()
        .map_err(|error| error.clone())
}

fn fetch_text(path: &str) -> Result<String, String> {
    let client = http_client()?;
    let mut errors = Vec::with_capacity(REPO_BASES.len());
    for base in REPO_BASES {
        let url = format!("{base}{path}");
        match client.get(&url).send() {
            Ok(response) if response.status().is_success() => match response.text() {
                Ok(text) => return Ok(text),
                Err(error) => errors.push(format!("{url}: 读取响应失败：{error}")),
            },
            Ok(response) => errors.push(format!("{url}: HTTP {}", response.status())),
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }
    Err(errors.join("；"))
}

/// 解析 `results/README.md` 的推荐列表：`- [型号](./来源/测量装置/型号目录)`。
pub(crate) fn parse_index(text: &str) -> Vec<AutoEqEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("- [") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once("](") else {
            continue;
        };
        let Some(dir) = tail.strip_suffix(')') else {
            continue;
        };
        let dir = dir.strip_prefix("./").unwrap_or(dir);
        let mut segments = dir.split('/');
        let (Some(source), Some(rig), Some(_model)) =
            (segments.next(), segments.next(), segments.next())
        else {
            continue;
        };
        if source.is_empty() || rig.is_empty() || name.is_empty() || !seen.insert(dir.to_string()) {
            continue;
        }
        let source = percent_decode(source);
        let category = category_for(&percent_decode(rig));
        let file = format!(
            "{dir}/{}",
            percent_encode_component(&format!("{name} ParametricEQ.txt"))
        );
        entries.push(AutoEqEntry {
            name: name.to_string(),
            search: format!("{} {}", name.to_lowercase(), source.to_lowercase())
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect(),
            source,
            category,
            file,
            bands: None,
            input_gain_db: 0.0,
            loading: false,
            error: None,
        });
    }
    entries
}

fn category_for(rig: &str) -> String {
    let rig = rig.to_ascii_lowercase();
    if rig.contains("over-ear") || rig.contains("over ear") {
        "头戴式".into()
    } else if rig.contains("earbud") {
        "耳塞式".into()
    } else if rig.contains("in-ear") || rig.contains("in ear") {
        "入耳式".into()
    } else {
        "其他".into()
    }
}

/// 解析 AutoEq 的 ParametricEQ 文本，返回频段与前级增益。
///
/// 格式示例：
/// ```text
/// Preamp: -6.1 dB
/// Filter 1: ON LSC Fc 105 Hz Gain 6.4 dB Q 0.70
/// Filter 2: ON PK Fc 8800 Hz Gain 5.1 dB Q 1.42
/// ```
pub(crate) fn parse_param_eq(text: &str) -> Result<(Vec<EqBand>, f32), String> {
    let mut preamp = None;
    let mut bands = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Preamp:") {
            preamp = leading_number(rest);
            continue;
        }
        if !line.starts_with("Filter") {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(fc) = tokens
            .iter()
            .position(|token| token.eq_ignore_ascii_case("Fc"))
        else {
            continue;
        };
        let Some((kind, is_q_shelf)) = fc
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
            .and_then(|token| parse_filter_kind(token))
        else {
            continue;
        };
        let Some(freq_hz) = number_at(&tokens, fc + 1) else {
            continue;
        };
        let gain_db = value_after(&tokens, "Gain").unwrap_or(0.0);
        let q = value_after(&tokens, "Q").unwrap_or(0.71);
        // `LSC`/`HSC` 的 Q 语义与播放引擎的架子斜率不同，换算后再交给
        // `EqBand`；其他滤波器 Q 直接沿用。
        let q = if is_q_shelf {
            shelf_slope(gain_db, q)
        } else {
            q
        };
        bands.push(
            EqBand {
                enabled: true,
                kind,
                freq_hz,
                gain_db,
                q,
            }
            .sanitized(),
        );
    }
    if bands.is_empty() {
        return Err("ParametricEQ 文件为空或格式不受支持".into());
    }
    let preamp =
        preamp.unwrap_or_else(|| -bands.iter().map(|band| band.gain_db).fold(0.0f32, f32::max));
    Ok((bands, preamp))
}

/// AutoEq 架子滤波器的 `Q` 转换为播放引擎使用的 RBJ 斜率 `S`。
///
/// AutoEq（`autoeq/peq.py`）用 `alpha = sin(w0) / (2Q)`；引擎的架子实现用
/// `alpha = sin(w0)/2 * sqrt((A + 1/A)(1/S - 1) + 2)`。令两者相等即可解出
/// 等效 S。AutoEq 最常用的 `Q 0.70` 约等于标准斜率 `S ≈ 0.98`。
fn shelf_slope(gain_db: f32, q: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let ratio = a + 1.0 / a;
    let q = q.clamp(0.1, 12.0);
    let inverse_slope = 1.0 + (1.0 / (q * q) - 2.0) / ratio;
    if inverse_slope.is_finite() && inverse_slope > 0.0 {
        (1.0 / inverse_slope).clamp(0.1, 4.0)
    } else {
        1.0
    }
}

fn parse_filter_kind(token: &str) -> Option<(FilterKind, bool)> {
    let is_q_shelf = token.eq_ignore_ascii_case("LSC") || token.eq_ignore_ascii_case("HSC");
    let kind = match token.to_ascii_uppercase().as_str() {
        "PK" | "PEAK" | "PEAKING" => FilterKind::Peaking,
        "LSC" | "LS" | "LSH" | "LOWSHELF" | "LOW_SHELF" => FilterKind::LowShelf,
        "HSC" | "HS" | "HSH" | "HIGHSHELF" | "HIGH_SHELF" => FilterKind::HighShelf,
        "LP" | "LOWPASS" | "LOW_PASS" => FilterKind::LowPass,
        "HP" | "HIGHPASS" | "HIGH_PASS" => FilterKind::HighPass,
        _ => return None,
    };
    Some((kind, is_q_shelf))
}

fn leading_number(token: &str) -> Option<f32> {
    token.split_whitespace().next()?.parse().ok()
}

fn number_at(tokens: &[&str], index: usize) -> Option<f32> {
    tokens.get(index)?.trim_end_matches(',').parse::<f32>().ok()
}

fn value_after(tokens: &[&str], key: &str) -> Option<f32> {
    tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case(key))
        .and_then(|index| number_at(tokens, index + 1))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX_SAMPLE: &str = "\
# Recommended Results
- [1Custom SA02](./crinacle/711%20in-ear/1Custom%20SA02)
- [Sennheiser HD 650](./oratory1990/over-ear/Sennheiser%20HD%20650)
- [Apple AirPods](./Rtings/HMS%20II.3%20earbud/Apple%20AirPods)
- [3040](./Rtings/Bruel%20&%20Kjaer%205128%20over-ear/3040)
- [full index](./INDEX.md)
";

    #[test]
    fn parses_recommended_index_into_real_repo_paths() {
        let entries = parse_index(INDEX_SAMPLE);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].name, "1Custom SA02");
        assert_eq!(entries[0].source, "crinacle");
        assert_eq!(entries[0].category, "入耳式");
        assert_eq!(
            entries[0].file,
            "crinacle/711%20in-ear/1Custom%20SA02/1Custom%20SA02%20ParametricEQ.txt"
        );
        assert_eq!(entries[1].category, "头戴式");
        assert_eq!(entries[1].source, "oratory1990");
        assert_eq!(entries[2].category, "耳塞式");
        // 目录里的 `&` 保持原样，文件名里的特殊字符会被编码。
        assert_eq!(entries[3].source, "Rtings");
        assert_eq!(entries[3].category, "头戴式");
        assert!(
            entries[3]
                .file
                .contains("Bruel%20&%20Kjaer%205128%20over-ear")
        );
        assert!(entries[3].file.ends_with("3040%20ParametricEQ.txt"));
    }

    #[test]
    fn parses_autoeq_parametric_eq_with_shelves() {
        let text = "\
Preamp: -6.1 dB
Filter 1: ON LSC Fc 105 Hz Gain 6.4 dB Q 0.70
Filter 2: ON PK Fc 8800 Hz Gain 5.1 dB Q 1.42
Filter 3: ON HSC Fc 10000 Hz Gain -2.1 dB Q 0.70
";
        let (bands, preamp) = parse_param_eq(text).unwrap();
        assert!((preamp + 6.1).abs() < 1e-4);
        assert_eq!(bands.len(), 3);
        assert_eq!(bands[0].kind, FilterKind::LowShelf);
        assert!((bands[0].freq_hz - 105.0).abs() < 1e-4);
        assert!((bands[0].gain_db - 6.4).abs() < 1e-4);
        assert_eq!(bands[1].kind, FilterKind::Peaking);
        assert_eq!(bands[2].kind, FilterKind::HighShelf);
        assert!((bands[2].gain_db + 2.1).abs() < 1e-4);
        // AutoEq 的架子 Q=0.70 换算到引擎斜率后接近 1.0。
        assert!((bands[0].q - 0.98).abs() < 0.03, "shelf q {}", bands[0].q);
        assert!((bands[2].q - 0.97).abs() < 0.03, "shelf q {}", bands[2].q);
    }

    #[test]
    fn missing_preamp_falls_back_to_headroom() {
        let text = "Filter 1: ON PK Fc 100 Hz Gain 5.0 dB Q 1.0\n";
        let (_, preamp) = parse_param_eq(text).unwrap();
        assert!((preamp + 5.0).abs() < 1e-4);
    }

    #[test]
    fn percent_helpers_roundtrip_spaces_and_symbols() {
        let encoded = percent_encode_component("Bruel & Kjaer (m20)");
        assert_eq!(encoded, "Bruel%20%26%20Kjaer%20%28m20%29");
        assert_eq!(percent_decode(&encoded), "Bruel & Kjaer (m20)");
        assert_eq!(
            percent_decode("crinacle%2F711%20in-ear"),
            "crinacle/711 in-ear"
        );
    }

    #[test]
    fn visible_indices_filter_category_and_query() {
        let entries = parse_index(INDEX_SAMPLE);
        let mut library = AutoEqLibrary::default();
        library.replace_entries(entries);
        assert_eq!(library.visible_indices("全部", "").len(), 4);
        assert_eq!(library.visible_indices("头戴式", "").len(), 2);
        assert_eq!(library.visible_indices("全部", "hd 650"), vec![1]);
        assert_eq!(library.visible_indices("全部", "hd650"), vec![1]);
        assert_eq!(library.visible_indices("入耳式", "apple").len(), 0);
    }

    /// 真实仓库联调：`cargo test -p chordal-ui live_repo -- --ignored`。
    #[test]
    #[ignore = "requires network / jaakkopasanen AutoEq repository"]
    fn live_repo_index_and_param_fetch() {
        let dir = std::env::temp_dir().join(format!("chordial-autoeq-live-{}", std::process::id()));
        let entries = fetch_index_into(&dir).expect("fetch AutoEq index");
        assert!(
            entries.len() > 5_000,
            "index only has {} entries",
            entries.len()
        );
        let target = entries
            .iter()
            .find(|entry| entry.name == "Sennheiser HD 650")
            .expect("Sennheiser HD 650 in recommended index");
        let (bands, preamp) = fetch_param_into(&dir, &target.file).expect("fetch ParametricEQ");
        assert!((3..=32).contains(&bands.len()), "{} bands", bands.len());
        assert!(preamp <= 0.0, "preamp {preamp}");
        // 路径里带 `&` 与括号的型号也要能正确编码/下载。
        for name in ["Aiaiai TMA-2 DJ", "AfterShokz Aeropex"] {
            let entry = entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("{name} in recommended index"));
            let (bands, _) =
                fetch_param_into(&dir, &entry.file).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!bands.is_empty(), "{name} has no bands");
        }
        assert!(load_cached_index_from(&dir).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

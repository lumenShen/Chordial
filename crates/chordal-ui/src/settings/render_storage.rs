//! 设置中心：render_storage（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    pub(super) fn render_storage(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let ratio = if self.cache_limit == 0 {
            0.0
        } else {
            (self.cache_used as f32 / self.cache_limit as f32).clamp(0.0, 1.0)
        };
        let bar_w = 236.0_f32;
        let usage = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_dim)
                    .child(format!(
                        "已用 {} / {} · {} 项",
                        format_bytes(self.cache_used),
                        format_bytes(self.cache_limit),
                        self.cache_entries
                    )),
            )
            .child(
                div()
                    .w(px(bar_w))
                    .h(px(6.0))
                    .rounded(px(3.0))
                    .bg(theme.border)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(px((bar_w * ratio).max(if self.cache_used > 0 {
                                2.0
                            } else {
                                0.0
                            })))
                            .rounded(px(3.0))
                            .bg(if ratio > 0.9 {
                                theme.danger
                            } else {
                                theme.text
                            }),
                    ),
            )
            .into_any_element();

        let presets = {
            let weak = weak.clone();
            let mut row = div().flex().items_center().gap(px(6.0));
            for (label, bytes) in [
                ("1 GB", 1024_u64 * 1024 * 1024),
                ("5 GB", 5 * 1024 * 1024 * 1024),
                ("10 GB", 10 * 1024 * 1024 * 1024),
                ("20 GB", 20 * 1024 * 1024 * 1024),
            ] {
                let weak = weak.clone();
                let active = self.cache_limit == bytes;
                row = row.child(chip(theme, label, active, move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.cache_limit = bytes;
                        view.emit(SettingsAction::SetCacheLimit(bytes), window, cx);
                        let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                        view.notice_ok(format!("缓存上限 {gb:.0} GB"), cx);
                    });
                }));
            }
            row = row
                .child(step_button(theme, "−", {
                    let weak = weak.clone();
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            let next = view
                                .cache_limit
                                .saturating_sub(1024 * 1024 * 1024)
                                .clamp(256 * 1024 * 1024, 100 * 1024 * 1024 * 1024);
                            view.cache_limit = next;
                            view.emit(SettingsAction::SetCacheLimit(next), window, cx);
                        });
                    }
                }))
                .child(step_button(theme, "+", {
                    let weak = weak.clone();
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            let next = view
                                .cache_limit
                                .saturating_add(1024 * 1024 * 1024)
                                .clamp(256 * 1024 * 1024, 100 * 1024 * 1024 * 1024);
                            view.cache_limit = next;
                            view.emit(SettingsAction::SetCacheLimit(next), window, cx);
                        });
                    }
                }));
            row.into_any_element()
        };

        let clear = {
            let weak = weak.clone();
            small_button(theme, "清空缓存", false, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.emit(SettingsAction::ClearCache, window, cx);
                    view.notice_ok("正在清空对象缓存…", cx);
                });
            })
        };

        let clear = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(clear)
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(theme.text_faint)
                    .child("缓存重建后播放不受影响"),
            )
            .into_any_element();

        // ---- 运行内存诊断 ----
        let mem = self.mem;
        let mem_value = |text: String| {
            div()
                .flex_shrink_0()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(text)
                .into_any_element()
        };
        let rss_text = mem
            .rss_bytes
            .map(chordal_core::mem::format_bytes)
            .unwrap_or_else(|| "不可用".to_string());
        let peak_rss_text = mem
            .peak_rss_bytes
            .map(chordal_core::mem::format_bytes)
            .unwrap_or_else(|| "不可用".to_string());
        let virtual_text = mem
            .virtual_bytes
            .map(chordal_core::mem::format_bytes)
            .unwrap_or_else(|| "不可用".to_string());
        let mut gauge_rows = Vec::with_capacity(Gauge::ALL.len());
        for gauge in Gauge::ALL {
            let label = match gauge {
                Gauge::AudioStream => "音频流缓冲",
                Gauge::Cover => "封面纹理",
                Gauge::Lyrics => "歌词模型",
                Gauge::Background => "背景光栅",
                Gauge::Library => "本地音乐库",
            };
            gauge_rows.push(setting_row(
                theme,
                label,
                None,
                mem_value(chordal_core::mem::format_bytes(mem.gauge(gauge) as u64)),
            ));
        }
        let memory = section(
            theme,
            "运行内存",
            Some("常驻内存来自操作系统；堆活跃由全局分配器统计；分项为业务侧主动登记的大块内存。"),
            vec![
                group(
                    theme,
                    vec![
                        setting_row(
                            theme,
                            "常驻内存",
                            Some(format!(
                                "峰值 {peak_rss_text} · 虚拟地址空间 {virtual_text}"
                            )),
                            mem_value(rss_text),
                        ),
                        setting_row(
                            theme,
                            "堆活跃",
                            Some(format!(
                                "峰值 {} · 分配 {} 次 / 释放 {} 次",
                                chordal_core::mem::format_bytes(mem.peak_live_bytes as u64),
                                mem.allocations,
                                mem.frees
                            )),
                            mem_value(chordal_core::mem::format_bytes(mem.live_bytes as u64)),
                        ),
                    ],
                ),
                group(theme, gauge_rows),
                hint(
                    theme,
                    "CHORDIAL_MEM_TRACE=1 把周期采样写入日志；CHORDIAL_MEM_INTERVAL_MS 调整间隔（默认 2000ms）；CHORDIAL_MEM_WARN_MB 设置常驻内存告警阈值。",
                ),
            ],
        );

        section(
            theme,
            "对象缓存",
            Some("封面、歌词、详情与直链的磁盘池；超出上限按 LRU 回收。"),
            vec![
                group(
                    theme,
                    vec![
                        setting_row(
                            theme,
                            "占用",
                            Some(format!("实际目录：{}", self.cache_root.display())),
                            usage,
                        ),
                        setting_row(
                            theme,
                            "上限",
                            Some("预设或按 1 GB 步进微调".into()),
                            presets,
                        ),
                        setting_block(
                            theme,
                            "缓存目录",
                            Some("按回车切换；目录不可写时自动回退临时目录".into()),
                            div().into_any_element(),
                            div()
                                .w_full()
                                .child(self.cache_dir_field.view.clone())
                                .into_any_element(),
                        ),
                    ],
                ),
                clear,
            ],
        )
        .child(memory)
        .child(section(
            theme,
            "下载",
            Some("显式下载的文件不占对象缓存配额，播放时优先使用。"),
            vec![group(
                theme,
                vec![setting_block(
                    theme,
                    "下载目录",
                    Some(format!("实际目录：{}", self.download_root.display())),
                    div().into_any_element(),
                    div()
                        .w_full()
                        .child(self.download_dir_field.view.clone())
                        .into_any_element(),
                )],
            )],
        ))
        .into_any_element()
    }
}

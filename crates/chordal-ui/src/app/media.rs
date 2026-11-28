//! `ChordialApp` 的 media 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn download_current(&mut self, cx: &mut Context<Self>) {
        // 队列里的条目是在线曲目；当前显示曲目可能已切换为本地同曲。
        let Some(track) = self
            .queue_pos
            .and_then(|pos| self.queue.get(pos).cloned())
            .or_else(|| self.current_track.clone())
        else {
            return;
        };
        if matches!(track.origin, TrackOrigin::Local { .. }) {
            self.info_message = Some("本地文件无需下载".into());
            cx.notify();
            return;
        }
        if self.downloads.path_for(&track.id).is_some() {
            self.info_message = Some("这首已经下载过了".into());
            cx.notify();
            return;
        }
        let Some(source) = self.registry.get(&track.id.source) else {
            self.status_message = Some("音源未加载，无法下载".into());
            cx.notify();
            return;
        };
        let downloads = self.downloads.clone();
        let title = track.title.clone();
        self.info_message = Some(format!("正在下载「{title}」…"));
        cx.notify();
        cx.spawn(async move |weak, cx| {
            // 搜索结果的专辑封面/序号可能缺失；下载前补一次详情，
            // 让写进文件的标签与在线页看到的一致。
            let detail = source.track(&track.id).await.ok();
            let track_for_download = detail.unwrap_or_else(|| track.clone());
            let source_for_lyrics = source.clone();
            let lyric_id = track.id.clone();
            let lyrics = match ChordialApp::run_background(async move {
                source_for_lyrics.lyrics(&lyric_id).await
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err(chordal_core::CoreError::Other("后台任务繁忙".into())),
            };
            let lyrics_lrc = lyrics
                .ok()
                .flatten()
                .and_then(|lyrics| lyrics_to_lrc(&lyrics));
            let cover_url = crate::lyrics::cover_url(&track_for_download);

            let resolved = source.resolve_stream(&track.id).await;
            let (url, format_hint) = match resolved {
                Ok(resolved) => {
                    let hint = resolved.quality.as_ref().and_then(|q| q.codec.clone());
                    match resolved.handle {
                        StreamHandle::Url(url) => (url, hint),
                        StreamHandle::File(_) => {
                            let _ = weak.update(cx, |app, cx| {
                                app.info_message = Some("该曲目已经指向本地文件".into());
                                cx.notify();
                            });
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.status_message = Some(format!("下载失败：{e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let result = match ChordialApp::run_background(async move {
                // 封面字节只取一次：嵌入标签 + 写边车都由 DownloadExtras 消费。
                let cover = cover_url.and_then(crate::lyrics::fetch_cover_raw_blocking);
                let extras = DownloadExtras {
                    cover,
                    lyrics: lyrics_lrc,
                };
                downloads.download_blocking(
                    &track_for_download,
                    &url,
                    format_hint.as_deref(),
                    &extras,
                )
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err("后台任务繁忙，请稍后重试".to_string()),
            };
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(path) => {
                        app.info_message = Some(format!("已下载：{}", path.display()));
                        app.status_message = None;
                        // 下载目录通常位于本地库根目录下：立即增量扫描，
                        // 让新文件带着标签出现在「本地音乐」里。
                        if let Some(local) = app.local_source.clone()
                            && local.roots().iter().any(|root| path.starts_with(root))
                        {
                            app.spawn_library_scan(local, cx);
                        }
                    }
                    Err(e) => app.status_message = Some(format!("下载失败：{e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- 歌词 / 封面 ----------

    /// 播放某曲目时异步补全元数据、歌词与封面。`lyrics_gen` 负责丢弃过期结果。
    pub(super) fn load_track_extras(&mut self, track: Track, cx: &mut Context<Self>) {
        self.lyrics_gen = self.lyrics_gen.wrapping_add(1);
        let generation = self.lyrics_gen;
        let registry = self.registry.clone();
        let view = self.lyric_view.clone();
        let player = self.player.clone();
        let app_cache = self.cache.clone();
        self.lyrics_status = Some("正在加载歌词…".into());
        cx.notify();

        cx.spawn(async move |weak, cx| {
            let Some(source) = registry.get(&track.id.source) else {
                let _ = weak.update(cx, |app, cx| {
                    if app.lyrics_gen == generation {
                        app.lyrics_status = Some("音源未加载，无法获取歌词".into());
                        cx.notify();
                    }
                });
                return;
            };

            // 远端源补一次 song/detail：拿到专辑封面 / 时长等完整字段。
            let is_remote = !matches!(track.origin, TrackOrigin::Local { .. });
            let detail = if is_remote {
                source.track(&track.id).await.ok()
            } else {
                None
            };
            let effective = detail.clone().unwrap_or_else(|| track.clone());
            let duration = effective.duration_ms.or(track.duration_ms);

            // 封面下载与歌词读取/转换并行：两者都提交到有界后台池，UI 只等待结果。
            let cover_cache = app_cache.clone();
            let cover_ns = track.id.source.clone();
            let cover_task = cover_url(&effective).map(|url| {
                let cover_cache = cover_cache.clone();
                let cover_ns = cover_ns.clone();
                ChordialApp::run_background(async move {
                    fetch_cover_blocking(Some(cover_cache), &cover_ns, url)
                })
            });
            let source_for_lyrics = source.clone();
            let lyric_id = track.id.clone();
            let lyrics_task = ChordialApp::run_background(async move {
                source_for_lyrics
                    .lyrics(&lyric_id)
                    .await
                    .map(|lyrics| lyrics.map(|lyrics| Self::prepare_lyrics(lyrics, duration)))
            });
            let (lyrics, cover) = futures::future::join(lyrics_task, async move {
                let cover: Option<(u32, u32, Vec<u8>)> = match cover_task {
                    Some(task) => task.await.ok().flatten(),
                    None => None,
                };
                cover
            })
            .await;
            let lyrics = match lyrics {
                Ok(result) => result,
                Err(_) => Err(chordal_core::CoreError::Other("后台任务繁忙".into())),
            };

            let _ = weak.update(cx, |app, cx| {
                if app.lyrics_gen != generation {
                    return;
                }
                if let Some(detail) = detail {
                    app.current_track = Some(detail);
                }
                if let Some((w, h, rgba)) = cover {
                    let image = Self::rgba_to_image(w, h, &rgba);
                    let old = app.cover_image.replace(image);
                    app.retire_image(old);
                    mem::set_gauge(Gauge::Cover, (w as usize) * (h as usize) * 4);
                    view.update(cx, |view, _| view.set_album_rgba(w, h, rgba));
                } else {
                    let old = app.cover_image.take();
                    app.retire_image(old);
                    mem::set_gauge(Gauge::Cover, 0);
                    view.update(cx, |view, _| view.clear_album());
                }

                match lyrics {
                    Ok(Some((lines, preview))) => {
                        app.lyric_preview = preview;
                        if lines.is_empty() {
                            view.update(cx, |view, _| view.clear_lyrics());
                            mem::set_gauge(Gauge::Lyrics, 0);
                            app.lyrics_status = Some("暂无歌词".into());
                        } else {
                            let count = lines.len();
                            let position = player.clock().0;
                            let bytes = estimate_lyrics_bytes(&lines, &app.lyric_preview);
                            mem::set_gauge(Gauge::Lyrics, bytes);
                            view.update(cx, |view, _| view.set_lyrics(lines, position));
                            app.lyrics_status = Some(format!("歌词 {count} 行"));
                        }
                    }
                    Ok(None) => {
                        view.update(cx, |view, _| view.clear_lyrics());
                        app.lyric_preview = Arc::new(Vec::new());
                        mem::set_gauge(Gauge::Lyrics, 0);
                        app.lyrics_status = Some("暂无歌词".into());
                    }
                    Err(err) => {
                        view.update(cx, |view, _| view.clear_lyrics());
                        app.lyric_preview = Arc::new(Vec::new());
                        mem::set_gauge(Gauge::Lyrics, 0);
                        app.lyrics_status = Some(format!("歌词加载失败：{err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn prepare_lyrics(
        lyrics: chordal_core::model::Lyrics,
        duration_ms: Option<u64>,
    ) -> (
        Vec<chordial_amll_gpui::model::LyricLine>,
        Arc<Vec<PreviewLyric>>,
    ) {
        let lines = to_amll_lyrics(&lyrics, duration_ms);
        let preview: Vec<PreviewLyric> = lines
            .iter()
            .filter_map(|line| {
                let text = line
                    .words
                    .iter()
                    .map(|word| word.word.as_str())
                    .collect::<String>();
                let text = SharedString::from(text.trim().to_string());
                let translation = SharedString::from(line.translated_lyric.trim().to_string());
                (!text.is_empty() || !translation.is_empty()).then_some(PreviewLyric {
                    start_ms: line.start_time,
                    text,
                    translation,
                })
            })
            .collect();
        (lines, Arc::new(preview))
    }

    /// 把不再绘制的图片排队，下一帧统一从 gpui sprite atlas 移除。
    ///
    /// `RenderImage` 的纹理一旦上传就不会随 `Arc` 释放自动回收；快速切歌时
    /// 每首封面都占一块图集，最终 `paint_image` 分配失败会表现为黑屏。
    pub(super) fn retire_image(&mut self, image: Option<Arc<RenderImage>>) {
        if let Some(image) = image {
            self.retired_images.push(image);
        }
    }

    /// RGBA → gpui 需要的 BGRA `RenderImage`（不预乘）。
    pub(super) fn rgba_to_image(w: u32, h: u32, rgba: &[u8]) -> Arc<RenderImage> {
        let mut bgra = rgba.to_vec();
        for px in bgra.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let image = image::RgbaImage::from_raw(w, h, bgra).expect("cover dimensions");
        Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
    }

    /// 列表用缩略图：封面源图已经限制在 512px，这里再缩到 72px 以压低
    /// 图集占用（列表可能同时展示几十张）。
    pub(super) fn rgba_to_thumb_image(w: u32, h: u32, rgba: &[u8]) -> Arc<RenderImage> {
        let src = image::RgbaImage::from_raw(w, h, rgba.to_vec()).expect("cover dimensions");
        let (tw, th) = if w.max(h) > TRACK_COVER_SIDE {
            let scale = TRACK_COVER_SIDE as f32 / w.max(h) as f32;
            (
                ((w as f32 * scale).round() as u32).max(1),
                ((h as f32 * scale).round() as u32).max(1),
            )
        } else {
            (w, h)
        };
        let thumb = if (tw, th) == (w, h) {
            src
        } else {
            image::imageops::resize(&src, tw, th, image::imageops::FilterType::Triangle)
        };
        let mut bgra = thumb.into_raw();
        for px in bgra.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let image = image::RgbaImage::from_raw(tw, th, bgra).expect("thumb dimensions");
        Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
    }

    /// 为虚拟列表当前可见的行请求封面。每张图只下载一次，失败也记下来，
    /// 否则来回滚动会对同一失效 URL 反复发起网络请求。
    pub(super) fn request_track_covers(
        &mut self,
        indices: &[usize],
        tracks: &[Track],
        cx: &mut Context<Self>,
    ) {
        let requests = indices
            .iter()
            .filter_map(|&index| {
                let track = tracks.get(index)?;
                let url = cover_url(track)?;
                Some((
                    track_cover_key_from_url(track, &url),
                    track.id.source.clone(),
                    url,
                ))
            })
            .collect();
        let to_load = self
            .track_covers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take_pending_requests(requests);
        self.start_track_cover_loads(to_load, cx);
    }

    pub(super) fn request_track_row_covers(
        &mut self,
        indices: &[usize],
        rows: &[TrackRowText],
        cx: &mut Context<Self>,
    ) {
        let to_load = {
            let mut covers = self.track_covers.lock().unwrap_or_else(|e| e.into_inner());
            let mut accepted = Vec::new();
            for &index in indices {
                if covers.pending.len() >= MAX_INFLIGHT_TRACK_COVERS {
                    break;
                }
                let Some(row) = rows.get(index) else {
                    continue;
                };
                let (Some(key), Some(url)) = (&row.cover_key, &row.cover_url) else {
                    continue;
                };
                if let Some(request) = covers.accept_pending_request(key, &row.id.source, url) {
                    accepted.push(request);
                }
            }
            accepted
        };
        self.start_track_cover_loads(to_load, cx);
    }

    fn start_track_cover_loads(
        &mut self,
        to_load: Vec<(String, SourceId, String)>,
        cx: &mut Context<Self>,
    ) {
        if to_load.is_empty() {
            return;
        }
        let cache = self.cache.clone();
        cx.spawn(async move |weak, cx| {
            for (key, source, url) in to_load {
                let cache = cache.clone();
                let source = source.clone();
                let url = url.clone();
                let result = ChordialApp::run_background(async move {
                    fetch_cover_blocking(Some(cache), &source, url)
                })
                .await
                .ok()
                .flatten();
                let _ = weak.update(cx, |app, cx| app.finish_track_cover(key, result, cx));
            }
        })
        .detach();
    }

    /// 封面下载完成：写入缩略图缓存（或记录失败），超限时回收最旧纹理。
    pub(super) fn finish_track_cover(
        &mut self,
        key: String,
        result: Option<(u32, u32, Vec<u8>)>,
        cx: &mut Context<Self>,
    ) {
        let mut retired = None;
        {
            let mut covers = self.track_covers.lock().unwrap_or_else(|e| e.into_inner());
            covers.pending.remove(&key);
            match result {
                Some((w, h, rgba)) => {
                    let image = Self::rgba_to_thumb_image(w, h, &rgba);
                    covers.failed.remove(&key);
                    covers.images.insert(key.clone(), image);
                    covers.order.push_back(key);
                    while covers.images.len() > MAX_TRACK_COVERS {
                        let Some(old) = covers.order.pop_front() else {
                            break;
                        };
                        retired = covers.images.remove(&old);
                        if retired.is_some() {
                            break;
                        }
                    }
                }
                None => {
                    covers.failed.insert(key);
                }
            }
        }
        self.retire_image(retired);
        cx.notify();
    }
}

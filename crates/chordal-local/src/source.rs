//! 本地源：source（由 lib.rs 拆分，行为不变）。

use super::tags::*;
use super::*;

#[async_trait]
impl MusicSource for LocalSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::local()
    }

    fn catalog(&self) -> Option<&dyn CatalogSource> {
        Some(self)
    }

    async fn search(&self, query: &SearchQuery) -> CoreResult<SearchResults> {
        let needle = query.normalized();
        let limit = query.limit.max(1) as usize;
        let offset = query.offset as usize;

        let scope = PerfScope::enter("local.search.filter");
        let index = self.index.read().expect("local index lock poisoned");
        let mut hits: Vec<&Track> = Vec::new();
        for track in index.tracks.iter() {
            // 预计算键上的单次 substring 匹配。提前终止：命中数达到
            // offset+limit 即止，典型复杂度 O(命中数·m) 而非 O(n·m)。
            if track.search_key.contains(&needle) {
                if hits.len() >= offset + limit {
                    break;
                }
                hits.push(track);
            }
        }
        scope.finish(index.tracks.len() as u64);

        let total = hits.len() as u64;
        let tracks = hits.into_iter().skip(offset).take(limit).cloned().collect();
        Ok(SearchResults {
            source: self.info.id.clone(),
            tracks,
            total: Some(total),
            elapsed_ms: None,
        })
    }

    async fn track(&self, id: &TrackId) -> CoreResult<Track> {
        let _scope = PerfScope::enter("local.track_lookup");
        let index = self.index.read().expect("local index lock poisoned");
        index
            .by_id
            .get(&id.id) // O(1)
            .and_then(|&idx| index.tracks.get(idx as usize))
            .cloned()
            .ok_or_else(|| CoreError::source_err(self.info.id.as_str(), format!("no track {id}")))
    }

    async fn stream(&self, id: &TrackId) -> CoreResult<StreamHandle> {
        let track = self.track(id).await?;
        match track.origin {
            TrackOrigin::Local { path } => Ok(StreamHandle::File(path)),
            _ => Err(CoreError::source_err(
                self.info.id.as_str(),
                "not a local track",
            )),
        }
    }

    async fn resolve_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        let track = self.track(id).await?;
        match track.origin {
            TrackOrigin::Local { path } => Ok(ResolvedStream {
                handle: StreamHandle::File(path),
                quality: track.quality_info,
            }),
            _ => Err(CoreError::source_err(
                self.info.id.as_str(),
                "not a local track",
            )),
        }
    }

    /// 同名边车歌词：`.lrc` / `.LRC` / `.txt`。一次命中即返回（O(1)）。
    /// 编码按 UTF-8(BOM) → UTF-16(BOM) → GBK 回退，兼容中文 LRC 生态。
    async fn lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let track = self.track(id).await?;
        let TrackOrigin::Local { path } = track.origin else {
            return Ok(None);
        };
        for ext in ["lrc", "LRC", "txt", "TXT"] {
            let candidate = path.with_extension(ext);
            if candidate == path {
                continue;
            }
            // 边车歌词不应把 UI 线程拖死：限制 4MB，避免误读大文本文件。
            let Ok(meta) = std::fs::metadata(&candidate) else {
                continue;
            };
            if meta.len() > 4 * 1024 * 1024 {
                continue;
            }
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            let raw = decode_lyric_bytes(&bytes);
            let mut lyrics = Lyrics::parse_lrc(&raw);
            if lyrics.lines.len() > 5_000 {
                lyrics.lines.truncate(5_000);
            }
            if !lyrics.lines.is_empty() {
                return Ok(Some(lyrics));
            }
        }
        Ok(None)
    }
}

#[async_trait]
#[async_trait]
impl CatalogSource for LocalSource {
    async fn collection(&self, id: &CollectionId) -> CoreResult<CollectionPage> {
        let _scope = PerfScope::enter("local.collection");
        let index = self.index.read().expect("local index lock poisoned");
        match id.kind {
            CollectionKind::Album => {
                let album = index.albums.get(&id.id).ok_or_else(|| {
                    CoreError::source_err(self.info.id.as_str(), format!("no album {id}"))
                })?;
                let tracks: Vec<Track> = album
                    .tracks
                    .iter()
                    .filter_map(|&i| index.tracks.get(i as usize).cloned())
                    .collect();
                let summary = CollectionSummary {
                    id: id.clone(),
                    title: album.title.clone(),
                    cover: album.cover.clone(),
                    creator: album.creator.clone(),
                    description: None,
                    track_count: Some(tracks.len() as u64),
                    year: album.year,
                    subscribed: false,
                    kind: CollectionKind::Album,
                };
                Ok(CollectionPage {
                    total: tracks.len() as u64,
                    summary: Some(summary),
                    tracks,
                    related_albums: Vec::new(),
                    offset: 0,
                    has_more: false,
                })
            }
            CollectionKind::Artist => {
                let artist = index.artists.get(&id.id).ok_or_else(|| {
                    CoreError::source_err(self.info.id.as_str(), format!("no artist {id}"))
                })?;
                let tracks: Vec<Track> = artist
                    .tracks
                    .iter()
                    .filter_map(|&i| index.tracks.get(i as usize).cloned())
                    .collect();
                let related_albums: Vec<CollectionSummary> = artist
                    .album_ids
                    .iter()
                    .filter_map(|album_id| {
                        let album = index.albums.get(album_id)?;
                        Some(CollectionSummary {
                            id: CollectionId::new(
                                self.info.id.clone(),
                                CollectionKind::Album,
                                album_id.clone(),
                            ),
                            title: album.title.clone(),
                            cover: album.cover.clone(),
                            creator: album.creator.clone(),
                            description: None,
                            track_count: Some(album.tracks.len() as u64),
                            year: album.year,
                            subscribed: false,
                            kind: CollectionKind::Album,
                        })
                    })
                    .collect();
                let summary = CollectionSummary {
                    id: id.clone(),
                    title: artist.name.clone(),
                    cover: artist.cover.clone(),
                    creator: None,
                    description: None,
                    track_count: Some(tracks.len() as u64),
                    year: None,
                    subscribed: false,
                    kind: CollectionKind::Artist,
                };
                Ok(CollectionPage {
                    total: tracks.len() as u64,
                    summary: Some(summary),
                    tracks,
                    related_albums,
                    offset: 0,
                    has_more: false,
                })
            }
            CollectionKind::Playlist => Err(CoreError::Config(
                "本地源暂不支持歌单页；请使用本地库视图".into(),
            )),
        }
    }

    async fn collection_tracks(
        &self,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage> {
        let page = self.collection(id).await?;
        let total = page.total;
        let tracks: Vec<Track> = page
            .tracks
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        Ok(TrackPage {
            has_more: offset as u64 + (tracks.len() as u64) < total,
            tracks,
            total,
            offset,
        })
    }

    async fn search_collections(&self, query: &SearchQuery) -> CoreResult<Vec<CollectionSummary>> {
        let needle = query.normalized();
        let limit = query.limit.max(1) as usize;
        let index = self.index.read().expect("local index lock poisoned");
        let mut out: Vec<CollectionSummary> = match query.kind {
            SearchKind::Album => index
                .albums
                .iter()
                .filter(|(_, album)| album.search_key.contains(&needle))
                .take(limit)
                .map(|(album_id, album)| CollectionSummary {
                    id: CollectionId::new(
                        self.info.id.clone(),
                        CollectionKind::Album,
                        album_id.clone(),
                    ),
                    title: album.title.clone(),
                    cover: album.cover.clone(),
                    creator: album.creator.clone(),
                    description: None,
                    track_count: Some(album.tracks.len() as u64),
                    year: album.year,
                    subscribed: false,
                    kind: CollectionKind::Album,
                })
                .collect(),
            SearchKind::Artist => index
                .artists
                .iter()
                .filter(|(_, artist)| artist.search_key.contains(&needle))
                .take(limit)
                .map(|(artist_id, artist)| CollectionSummary {
                    id: CollectionId::new(
                        self.info.id.clone(),
                        CollectionKind::Artist,
                        artist_id.clone(),
                    ),
                    title: artist.name.clone(),
                    cover: artist.cover.clone(),
                    creator: None,
                    description: None,
                    track_count: Some(artist.tracks.len() as u64),
                    year: None,
                    subscribed: false,
                    kind: CollectionKind::Artist,
                })
                .collect(),
            SearchKind::Track | SearchKind::Playlist => Vec::new(),
        };
        out.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(out)
    }
}

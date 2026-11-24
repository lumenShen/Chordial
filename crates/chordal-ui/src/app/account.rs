//! `ChordialApp` 的 account 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn spawn_account_bootstrap(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.netease_source() else {
            self.account_state = AccountState::Unknown;
            return;
        };
        let Some(account) = source.account() else {
            self.account_state = AccountState::Unsupported;
            return;
        };
        self.account_state = account.account_state();
        if account.cookie().is_none() {
            self.account_state = AccountState::LoggedOut;
            return;
        }
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            match account.refresh_account().await {
                Ok(Some(profile)) => {
                    let _ = weak.update(cx, |app, cx| {
                        if app.netease_source().is_none() {
                            return;
                        }
                        app.account_state = AccountState::LoggedIn(profile);
                        app.load_account_avatar(cx);
                        app.load_user_playlists(cx);
                        cx.notify();
                    });
                }
                Ok(None) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.account_state = AccountState::LoggedOut;
                        let old_avatar = app.account_avatar.take();
                        app.retire_image(old_avatar);
                        app.user_playlists = Arc::new(Vec::new());
                        app.save_cookie(None);
                        if app
                            .collection
                            .as_ref()
                            .is_some_and(|c| c.id.kind == CollectionKind::Playlist)
                        {
                            app.collection = None;
                            let old_cover = app.collection_cover.take();
                            app.retire_image(old_cover);
                            if app.view_tab == ViewTab::Collection {
                                app.view_tab = ViewTab::Library;
                            }
                        }
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.account_state = AccountState::Unknown;
                        app.info_message = Some(format!("账号校验失败：{e}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn spawn_netease_warmup(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.netease_source() else {
            return;
        };
        const PREPARING: &str = "正在准备网易云服务…首次运行会自动下载运行组件";
        self.info_message = Some(PREPARING.into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let result = source.warm_up().await;
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(()) => {
                        if app.info_message.as_deref() == Some(PREPARING) {
                            app.info_message = None;
                        }
                        // 旧版下载只写了音频流；服务就绪后原地补标签，
                        // 避免历史文件永远停留在“未知艺人/无封面/无歌词”。
                        app.spawn_download_metadata_repair(cx);
                    }
                    Err(e) => {
                        app.status_message = Some(format!("网易云服务准备失败：{e}"));
                        app.info_message = None;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 扫描下载索引，为缺少标题/艺人标签的历史下载补写元数据。
    ///
    /// 只处理“文件仍在、标签为空”的条目：重新取一次在线详情/歌词/封面，
    /// 复用下载落盘时的同一套写入逻辑，不重新下载音频。
    pub(super) fn spawn_download_metadata_repair(&mut self, cx: &mut Context<Self>) {
        let downloads = self.downloads.clone();
        let registry = self.registry.clone();
        cx.spawn(async move |weak, cx| {
            let pending = cx
                .background_executor()
                .spawn({
                    let downloads = downloads.clone();
                    async move { downloads.entries_missing_metadata() }
                })
                .await;
            if pending.is_empty() {
                return;
            }

            let mut repaired: Vec<PathBuf> = Vec::new();
            for (id, path) in pending {
                let Some(source) = registry.get(&id.source) else {
                    continue;
                };
                let Ok(track) = source.track(&id).await else {
                    continue;
                };
                let lyrics = source
                    .lyrics(&id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|lyrics| lyrics_to_lrc(&lyrics));
                let cover_url = crate::lyrics::cover_url(&track);
                let downloads = downloads.clone();
                let path_for_task = path.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let cover = cover_url.and_then(crate::lyrics::fetch_cover_raw_blocking);
                        let extras = DownloadExtras { cover, lyrics };
                        downloads.enrich_file_blocking(&path_for_task, &track, &extras)
                    })
                    .await;
                if result.is_ok() {
                    repaired.push(path);
                }
            }
            if repaired.is_empty() {
                return;
            }

            let count = repaired.len();
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("已为 {count} 首已下载曲目补齐元数据"));
                if let Some(local) = app.local_source.clone()
                    && repaired
                        .iter()
                        .any(|path| local.roots().iter().any(|root| path.starts_with(root)))
                {
                    app.spawn_library_scan(local, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn load_user_playlists(&mut self, cx: &mut Context<Self>) {
        if self.playlists_loading {
            return;
        }
        let Some(source) = self.netease_source() else {
            return;
        };
        if source.account().is_none() {
            return;
        }
        self.playlists_loading = true;
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            let result = account.user_playlists().await;
            let _ = weak.update(cx, |app, cx| {
                app.playlists_loading = false;
                if !matches!(app.account_state, AccountState::LoggedIn(_)) {
                    return;
                }
                match result {
                    Ok(list) => {
                        app.user_playlists = Arc::new(list);
                        app.status_message = None;
                        // 登录后“正在加载用户歌单”的提示在数据到位后要收掉。
                        if app.info_message.as_deref() == Some("已登录网易云，正在加载用户歌单")
                        {
                            app.info_message = None;
                        }
                    }
                    Err(e) => {
                        app.user_playlists = Arc::new(Vec::new());
                        app.info_message = Some(format!("用户歌单加载失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_login(&mut self, cx: &mut Context<Self>) {
        if self.login_source_id().is_none() {
            self.login_source = self.primary_netease_id();
        }
        self.login_open = true;
        self.login_tab = LoginTab::Qr;
        self.start_login(cx);
    }

    /// 从某个网易云实例的页面打开登录：浮层绑定到该实例，而不是默认实例。
    pub(super) fn open_login_for(&mut self, id: SourceId, cx: &mut Context<Self>) {
        self.login_source = Some(id);
        self.open_login(cx);
    }

    /// 登录成功后的实例级收尾：Cookie 写回该实例配置、清空快照并重新拉取。
    pub(super) fn finish_source_login(
        &mut self,
        source_id: &SourceId,
        profile: chordal_core::model::AccountProfile,
        cookie: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.set_source_cookie(source_id, cookie);
        self.source_user_playlists.remove(source_id);
        self.source_user_playlists_loading.remove(source_id);
        self.clear_source_feeds(source_id);
        // 全局账号态/头像只服务旧界面；以首个实例为准。
        if self.primary_netease_id().as_ref() == Some(source_id) {
            self.account_state = AccountState::LoggedIn(profile);
            self.load_account_avatar(cx);
            self.load_user_playlists(cx);
        }
        self.load_source_user_playlists(source_id, cx);
        self.ensure_source_feeds(source_id, cx);
    }

    pub(super) fn close_login(&mut self, cx: &mut Context<Self>) {
        self.login_open = false;
        self.login_gen = self.login_gen.wrapping_add(1);
        self.login_challenge = None;
        let old_qr = self.login_qr.take();
        self.retire_image(old_qr);
        self.login_status = None;
        self.login_captcha_input
            .update(cx, |state, cx| state.clear(cx));
        cx.notify();
    }

    pub(super) fn refresh_login(&mut self, cx: &mut Context<Self>) {
        if self.login_tab == LoginTab::Qr {
            self.start_login(cx);
        }
    }

    pub(super) fn switch_login_tab(&mut self, tab: LoginTab, cx: &mut Context<Self>) {
        if self.login_tab == tab {
            return;
        }
        self.login_tab = tab;
        self.login_gen = self.login_gen.wrapping_add(1);
        self.login_challenge = None;
        let old_qr = self.login_qr.take();
        self.retire_image(old_qr);
        self.login_status = None;
        match tab {
            LoginTab::Qr => self.start_login(cx),
            LoginTab::Phone => {
                self.login_status = Some("输入手机号并获取验证码".into());
                cx.notify();
            }
        }
    }

    pub(super) fn send_login_captcha(&mut self, cx: &mut Context<Self>) {
        if self
            .captcha_cooldown_until
            .is_some_and(|until| until > Instant::now())
        {
            return;
        }
        let phone = self.login_phone_input.read(cx).content().trim().to_string();
        if phone.chars().filter(|c| c.is_ascii_digit()).count() < 6 {
            self.login_status = Some("请输入正确的手机号".into());
            cx.notify();
            return;
        }
        let Some(source) = self.login_source() else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        self.login_status = Some(format!("正在向 {phone} 发送验证码…"));
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            match account.send_captcha(&phone).await {
                Ok(()) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some("验证码已发送".into());
                        app.captcha_cooldown_until = Some(Instant::now() + Duration::from_secs(60));
                        cx.notify();
                    });
                    cx.background_executor()
                        .timer(Duration::from_secs(61))
                        .await;
                    let _ = weak.update(cx, |app, cx| {
                        app.captcha_cooldown_until = None;
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some(format!("验证码发送失败：{e}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn submit_phone_login(&mut self, cx: &mut Context<Self>) {
        let phone = self.login_phone_input.read(cx).content().trim().to_string();
        let captcha = self
            .login_captcha_input
            .read(cx)
            .content()
            .trim()
            .to_string();
        if phone.is_empty() || captcha.is_empty() {
            self.login_status = Some("请输入手机号和验证码".into());
            cx.notify();
            return;
        }
        let Some(source_id) = self.login_source_id() else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        let Some(source) = self.registry.get(&source_id) else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        let generation = self.login_gen;
        self.login_status = Some("正在登录…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            match account.login_cellphone(&phone, &captcha).await {
                Ok(profile) => {
                    let cookie = account.cookie();
                    let _ = weak.update(cx, |app, cx| {
                        if app.login_gen != generation || !app.login_open {
                            return;
                        }
                        app.finish_source_login(&source_id, profile, cookie, cx);
                        app.login_open = false;
                        app.login_gen = app.login_gen.wrapping_add(1);
                        app.login_status = None;
                        let old_qr = app.login_qr.take();
                        app.retire_image(old_qr);
                        app.login_challenge = None;
                        app.login_captcha_input
                            .update(cx, |state, cx| state.clear(cx));
                        app.info_message = Some("已登录网易云，正在加载用户歌单".into());
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some(format!("登录失败：{e}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn load_account_avatar(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.account_state.profile().cloned() else {
            let old = self.account_avatar.take();
            self.retire_image(old);
            return;
        };
        let Some(url) = profile.avatar.clone() else {
            let old = self.account_avatar.take();
            self.retire_image(old);
            return;
        };
        let cache = self.cache.clone();
        // 头像属于账号数据：单独 namespace，登出/卸载时随账号缓存一起回收。
        let namespace = SourceId::new(format!("netease-account-{}", profile.uid));
        cx.spawn(async move |weak, cx| {
            let decoded = cx
                .background_executor()
                .spawn(async move { fetch_cover_blocking(Some(cache), &namespace, url) })
                .await;
            if let Some((w, h, rgba)) = decoded {
                let _ = weak.update(cx, |app, cx| {
                    let old = app.account_avatar.replace(Self::rgba_to_image(w, h, &rgba));
                    app.retire_image(old);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn start_login(&mut self, cx: &mut Context<Self>) {
        self.login_gen = self.login_gen.wrapping_add(1);
        let generation = self.login_gen;
        self.login_challenge = None;
        let old_qr = self.login_qr.take();
        self.retire_image(old_qr);
        self.login_status = Some("正在获取二维码…".into());
        cx.notify();

        let Some(source_id) = self.login_source_id() else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        let Some(source) = self.registry.get(&source_id) else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                let _ = weak.update(cx, |app, cx| {
                    app.login_status = Some("该音源不支持登录".into());
                    cx.notify();
                });
                return;
            };
            let challenge = match account.login_begin(LoginMethod::Qr).await {
                Ok(challenge) => challenge,
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some(format!("获取二维码失败：{e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let qr_url = challenge.qr_data_url.clone();
            let decoded = if let Some(url) = qr_url {
                cx.background_executor()
                    .spawn(async move { decode_data_url_image(&url) })
                    .await
            } else {
                None
            };
            let image = decoded.map(|(w, h, rgba)| Self::rgba_to_image(w, h, &rgba));
            let challenge_for_poll = challenge.clone();
            let ok = weak
                .update(cx, |app, cx| {
                    if app.login_gen != generation || !app.login_open {
                        return false;
                    }
                    app.login_challenge = Some(challenge);
                    let old_qr = std::mem::replace(&mut app.login_qr, image);
                    app.retire_image(old_qr);
                    app.login_status = Some(
                        if app.login_qr.is_some() {
                            "等待扫码"
                        } else {
                            "二维码图片解码失败，请刷新"
                        }
                        .into(),
                    );
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !ok {
                return;
            }

            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let alive = weak
                    .update(cx, |app, _| app.login_gen == generation && app.login_open)
                    .unwrap_or(false);
                if !alive {
                    return;
                }
                let progress = match account.login_poll(&challenge_for_poll).await {
                    Ok(progress) => progress,
                    Err(e) => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some(format!("登录轮询失败：{e}"));
                            cx.notify();
                        });
                        return;
                    }
                };
                match progress {
                    LoginProgress::Waiting => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some("等待扫码".into());
                            cx.notify();
                        });
                    }
                    LoginProgress::Scanned => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some("已扫描，请在手机确认".into());
                            cx.notify();
                        });
                    }
                    LoginProgress::Success(profile) => {
                        let cookie = account.cookie();
                        let _ = weak.update(cx, |app, cx| {
                            if app.login_gen != generation || !app.login_open {
                                return;
                            }
                            app.finish_source_login(&source_id, profile, cookie, cx);
                            app.login_open = false;
                            app.login_gen = app.login_gen.wrapping_add(1);
                            app.login_status = None;
                            let old_qr = app.login_qr.take();
                            app.retire_image(old_qr);
                            app.login_challenge = None;
                            app.info_message = Some("已登录网易云，正在加载用户歌单".into());
                            cx.notify();
                        });
                        return;
                    }
                    LoginProgress::Expired => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some("二维码已过期，点击刷新".into());
                            cx.notify();
                        });
                        return;
                    }
                    LoginProgress::Failed(e) => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some(format!("登录失败：{e}"));
                            cx.notify();
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }

    pub(super) fn logout_account(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.netease_source() else {
            return;
        };
        self.info_message = Some("正在退出登录…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            if let Some(account) = source.account() {
                let _ = account.logout().await;
            }
            let _ = weak.update(cx, |app, cx| {
                app.account_state = AccountState::LoggedOut;
                let old_avatar = app.account_avatar.take();
                app.retire_image(old_avatar);
                app.user_playlists = Arc::new(Vec::new());
                app.save_cookie(None);
                if app
                    .collection
                    .as_ref()
                    .is_some_and(|c| c.id.kind == CollectionKind::Playlist)
                {
                    app.collection = None;
                    let old_cover = app.collection_cover.take();
                    app.retire_image(old_cover);
                    app.view_tab = ViewTab::Library;
                }
                app.info_message = Some("已退出网易云账号".into());
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_collection(
        &mut self,
        id: CollectionId,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.collection_gen = self.collection_gen.wrapping_add(1);
        let generation = self.collection_gen;
        self.view_tab = ViewTab::Collection;
        self.source_route = None;
        // 手机壳里专辑/歌手/歌单是曲库页的下一层，不能在播放页背后悄悄
        // 换内容，否则返回时看起来「没有反应」。
        self.show_phone_browse();
        let old_cover = self.collection_cover.take();
        self.retire_image(old_cover);
        self.collection = Some(CollectionView {
            id: id.clone(),
            title: title.clone(),
            subtitle: "正在加载…".into(),
            creator: None,
            description: None,
            tracks: Arc::new(Vec::new()),
            related_albums: Arc::new(Vec::new()),
            loading: true,
            error: None,
        });
        cx.notify();

        let Some(source) = self.registry.get(&id.source) else {
            self.collection.as_mut().unwrap().error = Some("音源未加载".into());
            return;
        };
        if source.catalog().is_none() {
            self.collection.as_mut().unwrap().error = Some("该音源不支持集合页".into());
            return;
        }
        let cache = self.cache.clone();
        let source_id = id.source.clone();
        cx.spawn(async move |weak, cx| {
            let Some(catalog) = source.catalog() else {
                return;
            };
            let result = catalog.collection(&id).await;
            let _ = weak.update(cx, |app, cx| {
                if app.collection_gen != generation {
                    return;
                }
                let Some(view) = app.collection.as_mut() else {
                    return;
                };
                match result {
                    Ok(page) => {
                        let summary = page.summary;
                        if let Some(summary) = &summary {
                            view.title = summary.title.clone();
                            view.creator = summary.creator.clone();
                            view.description = summary.description.clone();
                            view.subtitle = format!(
                                "{} · {} 首{}",
                                summary.kind.label(),
                                page.total,
                                summary
                                    .creator
                                    .as_ref()
                                    .map(|c| format!(" · {c}"))
                                    .unwrap_or_default()
                            );
                        } else {
                            view.subtitle = format!("{} 首", page.total);
                        }
                        view.tracks = Arc::new(page.tracks);
                        view.related_albums = Arc::new(page.related_albums);
                        view.loading = false;
                        view.error = None;
                        let cover = summary.and_then(|s| s.cover);
                        if let Some(cover) = cover {
                            let cache = cache.clone();
                            let ns = source_id.clone();
                            cx.spawn(async move |weak, cx| {
                                let decoded =
                                    cx.background_executor()
                                        .spawn(async move {
                                            fetch_cover_blocking(Some(cache), &ns, cover)
                                        })
                                        .await;
                                if let Some((w, h, rgba)) = decoded {
                                    let _ = weak.update(cx, |app, cx| {
                                        if app.collection_gen == generation {
                                            let image = Self::rgba_to_image(w, h, &rgba);
                                            let old = app.collection_cover.replace(image);
                                            app.retire_image(old);
                                            cx.notify();
                                        }
                                    });
                                }
                            })
                            .detach();
                        }
                    }
                    Err(e) => {
                        view.loading = false;
                        view.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_album_for_current(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.current_track.clone() else {
            return;
        };
        let Some(album) = track.album else {
            return;
        };
        let Some(id) = album.id else {
            return;
        };
        self.open_collection(
            CollectionId::new(track.id.source.clone(), CollectionKind::Album, id),
            album.name,
            cx,
        );
    }

    pub(super) fn open_artist_for_current(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.current_track.clone() else {
            return;
        };
        let Some(artist) = track.artists.iter().find(|a| a.id.is_some()).cloned() else {
            return;
        };
        let Some(id) = artist.id else {
            return;
        };
        self.open_collection(
            CollectionId::new(track.id.source.clone(), CollectionKind::Artist, id),
            artist.name,
            cx,
        );
    }
}

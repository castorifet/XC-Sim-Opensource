xcsim_core_l10n::tl_file!("home");

use super::{
    load_font_with_cksum, set_bold_font, LibraryPage, MessagePage, NextPage, Page, ResPackPage, SFader, SettingsPage, SharedState,
    BOLD_FONT_CKSUM,
};
use crate::{
    anim::Anim,
    client::{recv_raw, Character, Client, LoginParams, User, UserManager},
    dir, get_data, get_data_mut,
    icons::Icons,
    login::Login,
    panel_bg,
    save_data,
    scene::{check_read_tos_and_policy, ProfileScene, JUST_LOADED_TOS},
    sync_data,
    threed::ThreeD,
};
use ::rand::{random, thread_rng, Rng};
use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use image::DynamicImage;
use macroquad::prelude::*;
use xcsim_core::{
    ext::{draw_parallelogram_ex, open_url, screen_aspect, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    info::ChartInfo,
    scene::{show_error, NextScene},
    task::Task,
    ui::{button_hit_large, ClipType, DRectButton, Dialog, FontArc, RectButton, Scroll, Ui},
};
use reqwest::StatusCode;
use serde::Deserialize;
use std::{
    borrow::Cow,
    sync::{atomic::Ordering, Arc},
};
use tap::Tap;
use tracing::{info, warn};

const BOARD_SWITCH_TIME: f32 = 4.;
const BOARD_TRANSIT_TIME: f32 = 1.2;

type BoldFontUpdateTask = Task<Result<Option<(FontArc, String)>>>;

#[derive(Deserialize)]
struct Version {
    version: semver::Version,
    date: NaiveDate,
    description: String,
    url: String,
}

pub struct HomePage {
    icons: Arc<Icons>,

    btn_play: DRectButton,
    btn_respack: DRectButton,
    btn_msg: DRectButton,
    btn_settings: DRectButton,
    btn_user: DRectButton,

    next_page: Option<NextPage>,

    login: Login,
    update_task: Option<Task<Result<User>>>,

    need_back: bool,
    sf: SFader,

    board_task: Option<Task<Result<Option<DynamicImage>>>>,
    board_last_time: f32,
    board_last: Option<String>,
    board_tex_last: Option<SafeTexture>,
    board_tex: Option<SafeTexture>,
    board_dir: bool,

    has_new_task: Option<Task<Result<bool>>>,
    has_new: bool,

    check_update_task: Option<Task<Result<Option<Version>>>>,
    check_bold_font_update_task: Option<BoldFontUpdateTask>,

    btn_play_3d: ThreeD,
    btn_other_3d: ThreeD,

    character: Character,
    char_appear_p: Anim<f32>,
    char_last_illu: Option<String>,
    char_last_user_id: Option<i32>,
    char_fetch_task: Option<Task<Result<Character>>>,
    char_illu: Option<SafeTexture>,
    char_illu_task: Option<Task<Result<DynamicImage>>>,

    char_screen_p: Anim<f32>,
    char_btn: RectButton,
    char_text_start: f32,
    char_cached_size: f32,
    char_scroll: Scroll,
    char_edit_btn: RectButton,

    enter_anim: Anim<f32>,
    first_in: bool,

    #[cfg(feature = "aa")]
    beian_btn: RectButton,
}

impl HomePage {
    pub async fn new() -> Result<Self> {
        let update_task = if get_data().config.offline_mode {
            None
        } else if let Some(u) = &get_data().me {
            UserManager::request(u.id);
            Some(Task::new(async {
                Client::login(LoginParams::RefreshToken {
                    token: &get_data().tokens.as_ref().unwrap().1,
                })
                .await?;
                Client::get_me().await
            }))
        } else {
            None
        };

        let flavor = match load_file("flavor").await.map(String::from_utf8) {
            Ok(Ok(flavor)) => flavor.trim().to_owned(),
            _ => "none".to_owned(),
        };

        let mut res = Self {
            icons: Arc::new(Icons::new().await?),

            btn_play: DRectButton::new().with_delta(-0.01).no_sound(),
            btn_respack: DRectButton::new().with_elevation(0.002).no_sound(),
            btn_msg: DRectButton::new().with_radius(0.008).with_delta(-0.003).with_elevation(0.002),
            btn_settings: DRectButton::new().with_radius(0.008).with_delta(-0.003).with_elevation(0.002),
            btn_user: DRectButton::new().with_delta(-0.003),

            next_page: None,

            login: Login::new(),
            update_task,

            need_back: false,
            sf: SFader::new(),

            board_task: None,
            board_last_time: f32::NEG_INFINITY,
            board_last: None,
            board_tex_last: None,
            board_tex: None,
            board_dir: false,

            has_new_task: None,
            has_new: false,

            check_update_task: Some(Task::new(async move {
                Ok(recv_raw(Client::get("/check-update").query(&[("version", env!("CARGO_PKG_VERSION")), ("flavor", &flavor)]))
                    .await?
                    .json()
                    .await?)
            })),
            check_bold_font_update_task: {
                let cksum = BOLD_FONT_CKSUM.with(|it| it.borrow().clone());
                Some(Task::new(async move {
                    let resp = Client::get("/font-bold").query(&[("cksum", cksum)]).send().await?;
                    if resp.status() == StatusCode::NOT_MODIFIED {
                        info!("bold font not modified");
                        return Ok(None);
                    }
                    if !resp.status().is_success() {
                        let status = resp.status().as_str().to_owned();
                        let text = resp.text().await.context("failed to receive text")?;
                        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(detail) = what["error"].as_str() {
                                bail!("request failed ({status}): {detail}");
                            }
                        }
                        bail!("request failed ({status}): {text}");
                    }
                    info!("downloading new bold font");
                    let bytes = resp.bytes().await?;
                    std::fs::write(dir::bold_font_path()?, &bytes).context("failed to save font")?;
                    Ok(Some(load_font_with_cksum(bytes.to_vec())?))
                }))
            },

            btn_play_3d: ThreeD::new(),
            btn_other_3d: ThreeD::new().tap_mut(|it| {
                it.anchor = vec2(0.2, -0.2);
                it.angle = 0.14;
                it.sync();
            }),

            character: get_data().character.clone().unwrap_or_default(),
            char_appear_p: Anim::new(0.),
            char_last_illu: None,
            char_last_user_id: None,
            char_fetch_task: None,
            char_illu: None,
            char_illu_task: None,
            char_screen_p: Anim::new(0.),
            char_btn: RectButton::new(),
            char_text_start: 0.,
            char_cached_size: 0.,
            char_scroll: Scroll::new().use_clip(ClipType::Clip),
            char_edit_btn: RectButton::new(),

            enter_anim: Anim::new(1.),
            first_in: true,

            #[cfg(feature = "aa")]
            beian_btn: RectButton::new(),
        };
        res.load_char_illu();

        Ok(res)
    }
}

impl HomePage {
    fn load_char_illu(&mut self) {
        let key = if self.character.illust == "@" {
            format!("@{}", self.character.id)
        } else {
            self.character.illust.clone()
        };
        if self.char_last_illu.as_ref() == Some(&key) {
            return;
        }
        self.char_last_illu = Some(key);

        self.char_appear_p.set(0.);

        #[cfg(closed)]
        if self.character.illust == "@" {
            let id = self.character.id.clone();
            self.char_illu_task =
                Some(Task::new(
                    async move { Ok(image::load_from_memory(&crate::inner::resolve_data(load_file(&format!("res/{id}.char")).await?))?) },
                ));
        } else {
            let file = crate::page::File {
                url: self.character.illust.clone(),
            };
            self.char_illu_task =
                Some(Task::new(async move { Ok(image::load_from_memory(&crate::inner::resolve_data(file.fetch().await?.to_vec()))?) }));
        }
    }

    fn fetch_has_new(&mut self) {
        if get_data().config.offline_mode || get_data().me.is_none() || get_data().tokens.is_none() {
            self.has_new_task = None;
            self.has_new = false;
            return;
        }
        let time = get_data().message_check_time.unwrap_or_default();
        self.has_new_task = Some(Task::new(async move {
            #[derive(Deserialize)]
            struct Resp {
                has: bool,
            }
            let resp: Resp = recv_raw(Client::get("/message/has_new").query(&[("checked", time)]))
                .await?
                .json()
                .await?;
            Ok(resp.has)
        }));
    }

    fn render_not_char(&mut self, ui: &mut Ui, s: &mut SharedState) {
        let t = s.t;

        let icon_play = self.icons.play.clone();
        let icon_respack = self.icons.respack.clone();
        let icon_settings = self.icons.settings.clone();
        let icon_msg = self.icons.msg.clone();
        let icon_user = self.icons.user.clone();
        let has_new = self.has_new;

        let ep = self.enter_anim.now(t).clamp(0., 1.);
        let enter_dx = (1.0 - ep) * -0.28;
        let enter_a = ep;
        let extra_tilt = (1.0 - ep) * 0.35;

        let char_cx = 0.0_f32;
        let char_cy = -0.02_f32;
        let char_r = 0.255_f32;

        let play_r = Rect::new(-0.57, -0.105, 0.31, 0.215);
        let respack_r = Rect::new(0.27, 0.02, 0.285, 0.17);
        let item_w = 0.225_f32;
        let item_h = 0.155_f32;
        let item_x = -0.88_f32;
        let messages_r = Rect::new(item_x, ui.top - 0.05 - item_h, item_w, item_h);
        let settings_r = Rect::new(item_x, messages_r.y - 0.015 - item_h, item_w, item_h);

        self.btn_play.inner.set(ui, play_r);
        self.btn_respack.inner.set(ui, respack_r);
        self.btn_settings.inner.set(ui, settings_r);
        self.btn_msg.inner.set(ui, messages_r);

        let play_p = self.btn_play.inner.touching();
        let respack_p = self.btn_respack.inner.touching();

        let tile_a = 0.92 * enter_a;
        let tile_base = Color::new(0.122, 0.063, 0.137, tile_a);
        let tile_press = Color::new(0.224, 0.118, 0.235, 0.96 * enter_a);
        let tile_top = Color::new(0.157, 0.086, 0.169, tile_a);
        let txt_c = Color::new(1., 1., 1., enter_a);

        let tile_angle = 0.32 + extra_tilt;

        s.render_fader(ui, |ui| {
            ui.fill_circle(char_cx, char_cy, char_r + 0.016, Color::new(0., 0., 0., 0.9 * enter_a));
            ui.fill_circle(char_cx, char_cy, char_r + 0.009, Color::new(1., 1., 1., enter_a));
            let avatar = get_data()
                .me
                .as_ref()
                .map(|user| UserManager::opt_avatar(user.id, &icon_user))
                .unwrap_or(Err(icon_user.clone()));
            ui.alpha(enter_a, |ui| {
                ui.avatar(char_cx, char_cy, char_r, t, avatar);
            });
        });
        self.char_btn.set(ui, Rect::new(char_cx - char_r, char_cy - char_r, char_r * 2., char_r * 2.));

        let mut vr = play_r;
        vr.x += enter_dx;
        let tilt_pt = vr.center() + vec2(-0.12, -0.4);
        let mat = ThreeD::build(tilt_pt, vr, tile_angle);
        s.render_fader(ui, |ui| {
            ui.with_gl(mat, |ui| {
                let top_c = if play_p { tile_press } else { tile_top };
                let bot_c = if play_p { tile_press } else { tile_base };
                draw_parallelogram_ex(vr, None, top_c, bot_c, true);
                draw_soft_text(ui, "Play", vr.x + 0.075, vr.y + 0.045, (0., 0.), 1.0, txt_c);
                let isz = 0.075_f32;
                let ir = Rect::new(vr.x + 0.055, vr.bottom() - isz - 0.03, isz, isz);
                ui.fill_rect(ir, (*icon_play, ir, ScaleType::Fit, Color::new(1., 0.58, 0.706, enter_a)));
            });
        });

        let mut vr = respack_r;
        vr.x += enter_dx;
        let tilt_pt = vr.center() + vec2(0.15, -0.4);
        let mat = ThreeD::build(tilt_pt, vr, tile_angle);
        s.render_fader(ui, |ui| {
            ui.with_gl(mat, |ui| {
                let top_c = if respack_p { tile_press } else { tile_top };
                let bot_c = if respack_p { tile_press } else { tile_base };
                draw_parallelogram_ex(vr, None, top_c, bot_c, true);
                draw_soft_text(ui, "Respack", vr.x + 0.06, vr.center().y, (0., 0.5), 0.82, txt_c);
                let isz = 0.06_f32;
                let ir = Rect::new(vr.x + 0.04, vr.bottom() - isz - 0.02, isz, isz);
                ui.fill_rect(ir, (*icon_respack, ir, ScaleType::Fit, Color::new(0.949, 0.412, 0.580, enter_a)));
            });
        });

        s.render_fader(ui, |ui| {
            self.btn_settings.render_shadow(ui, settings_r, t, |ui, path| {
                ui.fill_path(&path, panel_bg());
                let isz = 0.06_f32;
                let ir = Rect::new(settings_r.x + 0.02, settings_r.center().y - isz / 2. - 0.018, isz, isz);
                ui.fill_rect(ir, (*icon_settings, ir, ScaleType::Fit, crate::theme::cream_text(0.9)));
                ui.text("Settings")
                    .pos(settings_r.x + 0.02, settings_r.bottom() - 0.03)
                    .anchor(0., 1.)
                    .no_baseline()
                    .size(0.5)
                    .max_width(settings_r.w - 0.03)
                    .color(crate::theme::title_text())
                    .draw();
            });
        });

        s.render_fader(ui, |ui| {
            self.btn_msg.render_shadow(ui, messages_r, t, |ui, path| {
                ui.fill_path(&path, panel_bg());
                let isz = 0.06_f32;
                let ir = Rect::new(messages_r.x + 0.02, messages_r.center().y - isz / 2. - 0.018, isz, isz);
                ui.fill_rect(ir, (*icon_msg, ir, ScaleType::Fit, crate::theme::cream_text(0.9)));
                if has_new {
                    ui.fill_circle(ir.right() - 0.004, ir.y + 0.004, 0.009, RED);
                }
                ui.text("Messages")
                    .pos(messages_r.x + 0.02, messages_r.bottom() - 0.03)
                    .anchor(0., 1.)
                    .no_baseline()
                    .size(0.5)
                    .max_width(messages_r.w - 0.03)
                    .color(crate::theme::title_text())
                    .draw();
            });
        });
    }
}

fn draw_soft_text(ui: &mut Ui, text: &str, x: f32, y: f32, anchor: (f32, f32), size: f32, color: Color) -> Rect {
    const OFFS: [(f32, f32); 8] = [
        (-0.004, 0.), (0.004, 0.), (0., -0.004), (0., 0.004),
        (-0.003, -0.003), (0.003, -0.003), (-0.003, 0.003), (0.003, 0.003),
    ];
    let halo = Color::new(0., 0., 0., 0.18 * color.a);
    for (dx, dy) in OFFS {
        ui.text(text)
            .pos(x + dx, y + dy)
            .anchor(anchor.0, anchor.1)
            .no_baseline()
            .size(size)
            .color(halo)
            .draw();
    }
    ui.text(text)
        .pos(x, y)
        .anchor(anchor.0, anchor.1)
        .no_baseline()
        .size(size)
        .color(color)
        .draw()
}

impl Page for HomePage {
    fn label(&self) -> Cow<'static, str> {
        "".into()
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if self.need_back {
            self.sf.enter(s.t);
            self.need_back = false;
        }
        self.enter_anim.start(0., 1., s.t, 0.6);
        self.fetch_has_new();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.sf.transiting() {
            return Ok(true);
        }
        let t = s.t;
        let rt = s.rt;
        if self.login.touch(touch, s.t) {
            return Ok(true);
        }
        if self.char_screen_p.now(rt) < 1e-2 {
            self.btn_play_3d.touch(touch, t);
            if self.btn_play.touch(touch, t) {
                button_hit_large();
                self.next_page = Some(NextPage::Overlay(Box::new(LibraryPage::new(Arc::clone(&self.icons), s.icons.clone())?)));
                return Ok(true);
            }
            if self.btn_respack.touch(touch, t) {
                button_hit_large();
                self.next_page = Some(NextPage::Overlay(Box::new(ResPackPage::new(Arc::clone(&self.icons))?)));
                return Ok(true);
            }
            if self.btn_msg.touch(touch, t) {
                self.next_page = Some(NextPage::Overlay(Box::new(MessagePage::new())));
                return Ok(true);
            }
            if self.btn_settings.touch(touch, t) {
                self.next_page = Some(NextPage::Overlay(Box::new(SettingsPage::new(self.icons.icon.clone(), self.icons.lang.clone()))));
                return Ok(true);
            }
        } else {
            return Ok(false);
        }
        if self.btn_user.touch(touch, t) {
            if let Some(me) = &get_data().me {
                self.need_back = true;
                self.sf.goto(t, ProfileScene::new(me.id, self.icons.user.clone(), s.icons.clone()));
            } else {
                self.login.enter(t);
            }
            return Ok(true);
        }
        #[cfg(feature = "aa")]
        if self.beian_btn.touch(touch) {
            return Ok(true);
        }

        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.login.update(t)?;
        let current_user = Some(get_data().me.as_ref().map_or(-1, |it| it.id));
        self.char_scroll.update(t);
        if self.char_last_user_id != current_user {

self.char_fetch_task = None;
        }
        if let Some(task) = &mut self.update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {

                        if format!("{err:?}").contains("invalid token") {
                            get_data_mut().me = None;
                            get_data_mut().tokens = None;
                            let _ = save_data();
                            sync_data();
                        }

                        show_error(err.context(tl!("failed-to-update") + "\n" + tl!("note-try-login-again")));
                    }
                    Ok(val) => {
                        get_data_mut().me = Some(val);
                        save_data()?;
                    }
                }
                self.update_task = None;
            }
        }
        if self.board_task.is_none() && t - self.board_last_time > BOARD_SWITCH_TIME {
            let charts = &get_data().charts;
            let last_index = self
                .board_last
                .as_ref()
                .and_then(|path| charts.iter().position(|it| &it.local_path == path));
            if charts.is_empty() || (charts.len() == 1 && last_index.is_some()) {
                self.board_task = Some(Task::new(async move { Ok(None) }));
            } else {
                let mut index = thread_rng().gen_range(0..(charts.len() - last_index.is_some() as usize));
                if last_index.is_some_and(|it| it <= index) {
                    index += 1;
                }
                let path = charts[index].local_path.clone();
                let dir = xcsim_core::dir::Dir::new(format!("{}/{}", dir::charts()?, path))?;
                self.board_last = Some(path);
                self.board_task = Some(Task::new(async move {
                    let info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?)?;
                    let bytes = dir.read(info.illustration)?;
                    Ok(Some(image::load_from_memory(&bytes)?))
                }));
            }
        }
        if let Some(task) = &mut self.board_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to load illustration for board");
                    }
                    Ok(image) => {
                        if let Some(image) = image {
                            let tex: SafeTexture = image.into();
                            self.board_tex_last = self.board_tex.replace(tex);
                            self.board_dir = random();
                        }
                    }
                }
                self.board_last_time = t;
                self.board_task = None;
            }
        }
        if let Some(task) = &mut self.has_new_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to load has new {:?}", err);
                    }
                    Ok(has) => {
                        self.has_new = has;
                    }
                }
                self.has_new_task = None;
            }
        }
        if let Some(task) = &mut self.check_update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to check update {:?}", err);
                    }
                    Ok(Some(ver)) => {
                        if get_data().ignored_version.as_ref().is_none_or(|it| it < &ver.version) {
                            Dialog::plain(
                                tl!("update", "version" => ver.version.to_string()),
                                tl!("update-desc", "date" => ver.date.to_string(), "desc" => ver.description),
                            )
                            .buttons(vec![
                                ttl!("cancel").into_owned(),
                                tl!("update-ignore").into_owned(),
                                tl!("update-go").into_owned(),
                            ])
                            .listener(move |_dialog, pos| {
                                match pos {
                                    1 => {
                                        get_data_mut().ignored_version = Some(ver.version.clone());
                                        let _ = save_data();
                                    }
                                    2 => {
                                        let _ = open_url(&ver.url);
                                    }
                                    _ => {}
                                }
                                false
                            })
                            .show();
                        }
                    }
                    _ => {}
                }
                self.check_update_task = None;
            }
        }
        if let Some(task) = &mut self.check_bold_font_update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to check bold font update {:?}", err);
                    }
                    Ok(None) => {}
                    Ok(Some(parsed)) => {
                        info!(cksum = parsed.1, "new bold font");
                        set_bold_font(parsed);
                    }
                }
                self.check_bold_font_update_task = None;
            }
        }
        if let Some(task) = &mut self.char_illu_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "fail to load char illu");
                    }
                    Ok(image) => {
                        self.char_appear_p.goto(1., t, 0.5);
                        let tex: SafeTexture = image.into();
                        self.char_illu = Some(tex.with_mipmap());
                    }
                }
                self.char_illu_task = None;
            }
        }
        if let Some(task) = &mut self.char_fetch_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "fail to load char");
                    }
                    Ok(char) => {
                        info!(?char, "char loaded");
                        self.character = char;
                        get_data_mut().character = Some(self.character.clone());
                        let _ = save_data();
                        self.char_cached_size = 0.;
                        self.load_char_illu();
                    }
                }
                self.char_fetch_task = None;
            }
        }
        if JUST_LOADED_TOS.fetch_and(false, Ordering::Relaxed) {
            check_read_tos_and_policy(true, true);
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;

        if self.first_in {
            self.first_in = false;
            self.enter_anim.start(0., 1., t, 0.6);
        }

        let cp = self.char_screen_p.now(rt);
        s.render_fader(ui, |ui| {
            let r = Rect::new(-1. + 0.14 * cp, -ui.top + 0.12, 1., 1.7);
            if let Some(illu) = &self.char_illu {
                let p = self.char_appear_p.now(t);
                let (ox, oy, ow, oh) = self.character.illu_adjust;
                let r = Rect::new(r.x + ox, r.y + (1. - p) * 0.05 + oy, r.w + ow, r.h + oh);
                ui.fill_rect(ui.screen_rect(), (**illu, r, ScaleType::CropCenter, semi_white(p)));
            }
            self.char_btn.set(ui, r);

            if cp > 1e-5 {
                let height = 0.8 - ((screen_aspect() - 16. / 9.) * 0.2).min(0.2);
                let r = Rect::new(0.16, (-height - height * cp) / 4., 0.6, height);
                let mat = ThreeD::build(vec2(0., 0.), r, 0.12);
                let gl = unsafe { get_internal_gl() }.quad_gl;
                gl.push_model_matrix(mat);

                ui.alpha(cp, |ui| {
                    let mut r = Rect::new(r.x, r.y + 0.14, r.w, r.h - 0.14);
                    ui.fill_rect(r, semi_black(0.3));
                    ui.fill_rect(Rect::new(r.x, r.y, 0.01, r.h), WHITE);
                    let mut t = ui.text(tl!("change-char")).pos(r.x + 0.01, r.bottom() + 0.015).size(0.3);
                    let ir = t.measure().feather(0.007);
                    t.ui.fill_rect(ir, semi_black(0.2));
                    self.char_edit_btn.set(t.ui, ir);
                    t.draw();
                    let pad = 0.01;

                    let mut t = ui
                        .text(self.character.name_en())
                        .pos(r.right() - pad, r.bottom() - pad)
                        .anchor(1., 1.)
                        .color(semi_white(0.2));
                    if self.char_cached_size < 1e-6 {
                        let mut initial = 2.;
                        loop {
                            t = t.size(initial);
                            if t.measure().w < r.w * 0.7 {
                                break;
                            }
                            initial *= 0.95;
                        }
                        self.char_cached_size = initial;
                    } else {
                        t = t.size(self.char_cached_size);
                    }
                    t.draw();

                    r.x += 0.01;
                    r.w -= 0.01;

                    self.char_scroll.size((r.w, r.h));
                    ui.scope(|ui| {
                        ui.dx(r.x);
                        ui.dy(r.y);
                        let ow = r.w;
                        self.char_scroll.render(ui, |ui| {
                            let r = Rect::new(0., 0., r.w, r.h);
                            let r = r.feather(-0.03);
                            let r = ui.text(&self.character.intro).pos(r.x, r.y).max_width(r.w).multiline().size(0.4).draw();
                            (ow, r.h + 0.1)
                        });
                    });
                });

                let r = Rect::new(r.x, r.y, 0.4, 0.12);

                ui.alpha(cp, |ui| {
                    let r = ui
                        .text(&self.character.name)
                        .pos(r.x + (1. - cp) * 0.12 + 0.01, r.center().y)
                        .anchor(0., 0.5)
                        .size(self.character.name_size.unwrap_or(1.4))
                        .draw();

                    let off = if self.character.baseline { 0. } else { 0.01 };
                    ui.text(format!("Artist: {}", self.character.artist))
                        .pos(r.right() + (1. - cp) * 0.1 + 0.02, r.bottom() + off - 0.03)
                        .anchor(0., 1.)
                        .size(0.34)
                        .color(semi_white(0.7))
                        .draw();
                    ui.text(format!("Designer: {}", self.character.designer))
                        .pos(r.right() + (1. - cp) * 0.1 + 0.016, r.bottom() + off)
                        .anchor(0., 1.)
                        .size(0.34)
                        .color(semi_white(0.7))
                        .draw();
                });

                gl.pop_model_matrix();
            }
        });

        ui.alpha(1. - cp, |ui| {
            self.render_not_char(ui, s);
        });

        s.fader.roll_back();
        s.render_fader(ui, |ui| {
            let top = ui.top;

            let rad = 0.045_f32;
            let ct = (-1. + 0.09, -top + 0.10);
            self.btn_user.config.radius = rad;
            let r = Rect::new(ct.0, ct.1, 0., 0.).feather(rad);
            self.btn_user.build(ui, t, r, |ui, _| {
                ui.avatar(
                    ct.0,
                    ct.1,
                    r.w / 2.,
                    t,
                    get_data()
                        .me
                        .as_ref()
                        .map(|user| UserManager::opt_avatar(user.id, &self.icons.user))
                        .unwrap_or(Err(self.icons.user.clone())),
                );
            });

            let info_x = ct.0 + rad + 0.03;
            if let Some(me) = &get_data().me {
                let name = me.name.clone();
                let rks = me.rks;
                draw_soft_text(ui, &name, info_x, ct.1 - 0.004, (0., 1.), 0.56, crate::theme::title_text());
                draw_soft_text(ui, &format!("RKS {:.2}", rks), info_x, ct.1 + 0.006, (0., 0.), 0.4, crate::theme::FIREFLY_PINK_SOFT);
            } else {
                draw_soft_text(ui, &tl!("not-logged-in"), info_x, ct.1, (0., 0.5), 0.52, crate::theme::cream_text(0.95));
            }

            let title_y = -top + 0.235;
            let mut x = -1. + 0.05;
            x = draw_soft_text(ui, "XC-SIM", x, title_y, (0., 0.5), 0.86, crate::theme::FIREFLY_PINK).right();
            x = draw_soft_text(ui, " - Code By ", x, title_y, (0., 0.5), 0.86, crate::theme::cream_text(0.95)).right();
            draw_soft_text(ui, "ながよづき", x, title_y, (0., 0.5), 0.86, crate::theme::FIREFLY_PINK_SOFT);

            #[cfg(feature = "aa")]
            {
                let r = ui.screen_rect();
                let r = ui
                    .text("备案号：闽ICP备18008307号-64A")
                    .pos(r.x + 0.02, r.bottom() - 0.03)
                    .size(0.5)
                    .anchor(0., 1.)
                    .draw();
                self.beian_btn.set(ui, r);
            }
        });

        self.login.render(ui, t);
        self.sf.render(ui, t);

        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, s: &mut SharedState) -> NextScene {
        self.sf.next_scene(s.t).unwrap_or_default()
    }
}

xcsim_core_l10n::tl_file!("respack");

use super::{Page, SharedState};
use crate::{
    dir, get_data, get_data_mut,
    icons::Icons,
    save_data,
    scene::{confirm_delete, MainScene},
};
use anyhow::Result;
use macroquad::prelude::*;

const XHUS_ACCENT: Color = crate::theme::FIREFLY_PINK_DEEP;

use xcsim_core::{
    core::{NoteStyle, ParticleEmitter, ResPackInfo, ResourcePack},
    ext::{create_audio_manger, poll_future, semi_white, LocalTask, RectExt, SafeTexture, ScaleType},
    scene::{request_file, show_error, show_message},
    ui::{DRectButton, Dialog, Scroll, Ui},
};
use sasa::{AudioManager, PlaySfxParams, Sfx};
use serde_yaml::Error;
use std::{
    borrow::Cow,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

fn build_emitter(pack: &ResourcePack) -> Result<ParticleEmitter> {
    ParticleEmitter::new(pack, get_data().config.note_scale * 0.6, pack.info.hide_particles)
}

pub struct ResPackItem {
    path: Option<PathBuf>,
    name: String,
    btn: DRectButton,

    loaded: Option<ResourcePack>,
    load_task: LocalTask<Result<ResourcePack>>,
}

impl ResPackItem {
    pub fn new(path: Option<PathBuf>, name: String) -> Self {
        Self {
            path,
            name,
            btn: DRectButton::new(),

            loaded: None,
            load_task: None,
        }
    }

    fn load(&mut self) {
        if let Some(loaded) = self.loaded.take() {
            self.load_task = Some(Box::pin(async move { Ok(loaded) }));
        } else {
            self.load_task = Some(Box::pin(ResourcePack::from_path(self.path.clone())));
        }
    }
}

pub struct ResPackPage {
    audio: AudioManager,
    items: Vec<ResPackItem>,
    import_btn: DRectButton,
    btns_scroll: Scroll,
    index: usize,

    icons: Arc<Icons>,

    info_btn: DRectButton,
    delete_btn: DRectButton,

    should_delete: Arc<AtomicBool>,

    emitter: Option<ParticleEmitter>,
    sfxs: Option<[Sfx; 3]>,
    last_round: u32,
}

impl ResPackPage {
    pub fn new(icons: Arc<Icons>) -> Result<Self> {
        MainScene::take_imported_respack();
        let dir = dir::respacks()?;
        let mut items = vec![ResPackItem::new(None, tl!("default").into_owned())];
        let data = get_data_mut();
        data.respacks = data
            .respacks
            .clone()
            .into_iter()
            .filter(|path| -> bool {
                let p = format!("{dir}/{path}");
                let p = Path::new(&p);
                if !p.is_dir() {
                    return false;
                }
                let cfg = File::open(p.join("info.yml"));
                match cfg {
                    Err(_) => {
                        let _ = std::fs::remove_dir_all(p);
                        false
                    }
                    Ok(cfg) => {
                        let info: Result<ResPackInfo, Error> = serde_yaml::from_reader(cfg);
                        match info {
                            Err(_) => {
                                let _ = std::fs::remove_dir_all(p);
                                false
                            }
                            Ok(info) => {
                                items.push(ResPackItem::new(Some(p.to_owned()), info.name));
                                true
                            }
                        }
                    }
                }
            })
            .collect();
        save_data()?;

        let index = get_data().respack_id;
        items[index].load();
        let delete_btn = DRectButton::new().with_delta(-0.004).with_elevation(0.);
        Ok(Self {
            audio: create_audio_manger(&get_data().config)?,
            items,
            import_btn: DRectButton::new(),
            btns_scroll: Scroll::new(),
            index,

            icons,

            info_btn: delete_btn.clone(),
            delete_btn,

            should_delete: Arc::new(AtomicBool::default()),

            emitter: None,
            sfxs: None,
            last_round: u32::MAX,
        })
    }
}

impl Page for ResPackPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn custom_title(&self) -> bool { true }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.btns_scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.import_btn.touch(touch, t) {
            request_file("_import_respack");
            return Ok(true);
        }
        if self.items[self.index].load_task.is_none() {
            for (index, item) in self.items.iter_mut().enumerate() {
                if item.btn.touch(touch, t) {
                    self.index = index;
                    get_data_mut().respack_id = index;
                    save_data()?;
                    item.load();
                    return Ok(true);
                }
            }
        }
        if self.info_btn.touch(touch, t) {
            let item = &self.items[self.index];
            let info = &item.loaded.as_ref().unwrap().info;
            Dialog::plain(
                tl!("info"),
                tl!("info-content", "name" => item.name.clone(), "author" => info.author.clone(), "desc" => info.description.clone()),
            )
            .listener(|_dialog, pos| pos == -2)
            .show();
            return Ok(true);
        }
        if self.delete_btn.touch(touch, t) {
            if self.index == 0 {
                show_message(tl!("cant-delete-builtin")).error();
                return Ok(true);
            }
            confirm_delete(self.should_delete.clone());
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.btns_scroll.update(t);
        let item = &mut self.items[self.index];
        if let Some(task) = &mut item.load_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-failed")));
                    }
                    Ok(val) => {
                        self.emitter = Some(build_emitter(&val)?);
                        self.sfxs = Some([
                            self.audio.create_sfx(val.sfx_click.clone(), None)?,
                            self.audio.create_sfx(val.sfx_drag.clone(), None)?,
                            self.audio.create_sfx(val.sfx_flick.clone(), None)?,
                        ]);
                        item.loaded = Some(val);
                    }
                }
                item.load_task = None;
            }
        }
        if self.should_delete.fetch_and(false, Ordering::Relaxed) {
            std::fs::remove_dir_all(self.items[self.index].path.as_ref().unwrap())?;
            self.items.remove(self.index);
            get_data_mut().respacks.remove(self.index - 1);
            self.index -= 1;
            get_data_mut().respack_id = self.index;
            save_data()?;
            self.items[self.index].load();
            show_message(tl!("deleted")).ok();
        }
        if let Some(item) = MainScene::take_imported_respack() {
            self.items.push(item);
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;

        let accent     = XHUS_ACCENT;
        let body_bg    = Color::new(0.165, 0.110, 0.180, 0.97);
        let sidebar_bg = Color::new(0.110, 0.071, 0.125, 0.97);
        let dark_text  = Color::new(0.984, 0.973, 0.886, 1.);
        let muted_text = Color::new(1.0,   0.776, 0.847, 0.60);
        let sep_c      = Color::new(1.0,   0.776, 0.847, 0.12);
        let border_c   = Color::new(1.0,   0.776, 0.847, 0.32);
        let title_h    = 0.085_f32;

        let mut cr = ui.content_rect();
        let d = 0.29;
        cr.x += d;
        cr.w -= d;
        let r = Rect::new(-0.92, cr.y, 0.47, cr.h);


        s.render_fader(ui, |ui| {
            let sidebar_path = r.rounded(0.005);
            ui.fill_path(&sidebar_path, sidebar_bg);
            ui.stroke_path(&sidebar_path, 0.003, border_c);


            let title_r = Rect::new(r.x, r.y, r.w, title_h);
            ui.fill_rect(title_r, accent);
            ui.text(tl!("label"))
                .pos(r.x + 0.015, title_r.center().y)
                .anchor(0., 0.5).no_baseline().size(0.44).color(WHITE)
                .draw();

            let pad = 0.014;
            let item_h = 0.09_f32;
            self.btns_scroll.size((r.w, r.h - title_h - pad));
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y + title_h + pad);
                self.btns_scroll.render(ui, |ui| {
                    let w = r.w - pad * 2.;
                    let mut h = 0.;
                    let count = self.items.len();
                    for (index, item) in self.items.iter_mut().enumerate() {
                        let ir = Rect::new(pad, 0., r.w - pad * 2., item_h);
                        let chosen = index == self.index;
                        if chosen {
                            let sel_bg = Color::new(accent.r, accent.g, accent.b, 0.18);
                            ui.fill_rect(ir, sel_bg);
                            ui.fill_rect(Rect::new(ir.x, ir.y, 0.005, ir.h), accent);
                        }
                        item.btn.render_text_left(ui, ir, t, 1., &item.name, 0.5, chosen);
                        if index + 1 < count {
                            ui.fill_rect(Rect::new(pad, item_h - 0.001, w, 0.002), sep_c);
                        }
                        ui.dy(item_h + pad);
                        h += item_h + pad;
                    }

                    ui.fill_rect(Rect::new(pad, -0.002, w, 0.002), sep_c);
                    let ir = Rect::new(pad, 0., r.w - pad * 2., item_h);
                    self.import_btn.render_text_left(ui, ir, t, 1., "+ Import", 0.5, false);
                    h += item_h + pad * 2.;
                    (w, h)
                });
            });
        });


        let item_loaded = self.items[self.index].loaded.is_some();
        s.render_fader(ui, |ui| {
            let content_path = cr.rounded(0.005);
            ui.fill_path(&content_path, body_bg);
            ui.stroke_path(&content_path, 0.003, border_c);


            let title_r = Rect::new(cr.x, cr.y, cr.w, title_h);
            ui.fill_rect(title_r, accent);
            let item = &self.items[self.index];
            ui.text(&item.name)
                .pos(cr.x + 0.02, title_r.center().y)
                .anchor(0., 0.5).no_baseline().size(0.44).color(WHITE)
                .draw();

            if let Some(pack) = &item.loaded {

                let sl_y = cr.y + title_h + 0.025;
                ui.text("Preview")
                    .pos(cr.x + 0.025, sl_y)
                    .anchor(0., 0.).no_baseline().size(0.38)
                    .color(muted_text)
                    .draw();
                ui.fill_rect(Rect::new(cr.x + 0.025, sl_y + 0.038, cr.w - 0.05, 0.002), border_c);


                let width = 0.16;
                let preview_y = cr.y + title_h + 0.09;
                let mut r = Rect::new(cr.x + 0.07, preview_y, width, 0.);
                let mut draw = |mut r: Rect, tex: Texture2D, mh: Texture2D| {
                    let y = r.y;
                    r.h = tex.height() / tex.width() * r.w;
                    r.y = y - r.h / 2.;
                    ui.fill_rect(r, (tex, r, ScaleType::Fit));
                    r.x += r.w * 1.8;
                    r.w *= mh.width() / tex.width();
                    r.x -= r.w / 2.;
                    r.h = mh.height() / mh.width() * r.w;
                    r.y = y - r.h / 2.;
                    ui.fill_rect(r, (mh, r, ScaleType::Fit));
                };
                let avail_h = cr.h - title_h - 0.09 - 0.1;
                let sp = (avail_h - 0.1) / 2.;
                draw(r, *pack.note_style.click, *pack.note_style_mh.click);
                r.y += sp;
                draw(r, *pack.note_style.drag, *pack.note_style_mh.drag);
                r.y += sp;
                draw(r, *pack.note_style.flick, *pack.note_style_mh.flick);


                let hold_x = cr.x + cr.w * 0.32;
                let mut r = Rect::new(hold_x, preview_y, width, avail_h - 0.05);
                let draw = |mut r: Rect, style: &NoteStyle, width: f32| {
                    let conv = |r: Rect, tex: &SafeTexture| Rect::new(r.x * tex.width(), r.y * tex.height(), r.w * tex.width(), r.h * tex.height());
                    let tr = conv(style.hold_tail_rect(), &style.hold);
                    let factor = if pack.info.hold_compact { 0.5 } else { 1. };
                    let h = tr.h / tr.w * width;
                    let r2 = Rect::new(r.x, r.y - h * factor, width, h);
                    let r2 = ui.rect_to_global(r2);
                    draw_texture_ex(
                        *style.hold,
                        r2.x, r2.y,
                        semi_white(ui.alpha),
                        DrawTextureParams { source: Some(tr), dest_size: Some(vec2(r2.w, r2.h)), ..Default::default() },
                    );
                    let tr = conv(style.hold_head_rect(), &style.hold);
                    let h = tr.h / tr.w * width;
                    let r2 = Rect::new(r.x, r.bottom() - h * (1. - factor), width, h);
                    let r2 = ui.rect_to_global(r2);
                    draw_texture_ex(
                        *style.hold,
                        r2.x, r2.y,
                        semi_white(ui.alpha),
                        DrawTextureParams { source: Some(tr), dest_size: Some(vec2(r2.w, r2.h)), ..Default::default() },
                    );
                    r.w = width;
                    let r2 = ui.rect_to_global(r);
                    draw_texture_ex(
                        if pack.info.hold_repeat { **style.hold_body.as_ref().unwrap() } else { *style.hold },
                        r2.x, r2.y,
                        semi_white(ui.alpha),
                        DrawTextureParams {
                            source: Some(if pack.info.hold_repeat {
                                let hold_body = style.hold_body.as_ref().unwrap();
                                let w = hold_body.width();
                                Rect::new(0., 0., w, r2.h / width / 2. * w)
                            } else {
                                conv(style.hold_body_rect(), &style.hold)
                            }),
                            dest_size: Some(vec2(r2.w, r2.h)),
                            ..Default::default()
                        },
                    )
                };
                draw(r, &pack.note_style, width);
                r.x += width + 0.04;
                draw(r, &pack.note_style_mh, width * pack.note_style_mh.hold.width() / pack.note_style.hold.width());


                if let Some(emitter) = &mut self.emitter {
                    emitter.draw(get_frame_time());
                }


                let inter = 1.5;
                let rnd = t.div_euclid(inter);
                let irnd = rnd as u32;
                let tex = match irnd % 3 {
                    0 => *pack.note_style.click,
                    1 => *pack.note_style.drag,
                    2 => *pack.note_style.flick,
                    _ => unreachable!(),
                };
                let anim_cx = cr.x + cr.w * 0.72;
                let anim_top = preview_y;
                let anim_bot = cr.bottom() - 0.12;
                let line_y = anim_top + (anim_bot - anim_top) * 0.72;
                ui.fill_rect(Rect::new(anim_cx - 0.18, line_y - 0.004, 0.36, 0.008), Color::new(0.984, 0.973, 0.886, 0.9));
                let p = (t - inter * rnd) / 0.9;
                if p <= 1. {
                    let y = anim_top + (line_y - anim_top) * p;
                    let h = tex.height() / tex.width() * width;
                    let r = Rect::new(anim_cx - width / 2., y - h / 2., width, h);
                    ui.fill_rect(r, (tex, r, ScaleType::Fit));
                } else if irnd != self.last_round {
                    if let Some(emitter) = &mut self.emitter {
                        emitter.emit_at(vec2(anim_cx, line_y), 0., pack.info.fx_perfect());
                    }
                    if let Some(sfxs) = &mut self.sfxs {
                        let _ = sfxs[(irnd % 3) as usize].play(PlaySfxParams::default());
                    }
                    self.last_round = irnd;
                }


                let lx = cr.x + 0.025;
                ui.text(&item.name)
                    .pos(lx, cr.bottom() - 0.055)
                    .anchor(0., 1.)
                    .max_width(cr.right() - lx - 0.3)
                    .size(0.9)
                    .color(dark_text)
                    .draw();
            } else {
                let ct = cr.center();
                ui.loading(ct.x, ct.y, t, WHITE, ());
            }


            let btn_s = 0.10_f32;
            let btn_pad = 0.035_f32;
            let btn_bg = Color::new(0.243, 0.165, 0.255, 1.);
            let mut bx = cr.right() - btn_pad - btn_s;
            let by = cr.bottom() - btn_pad - btn_s;
            let tr = Rect::new(bx, by, btn_s, btn_s);
            self.delete_btn.render_shadow(ui, tr, t, |ui, path| {
                ui.fill_path(&path, btn_bg);
                let r = tr.feather(-0.018);
                ui.fill_rect(r, (*self.icons.delete, r, ScaleType::Fit));
            });
            if item_loaded {
                bx -= btn_s + 0.012;
                let tr = Rect::new(bx, by, btn_s, btn_s);
                self.info_btn.render_shadow(ui, tr, t, |ui, path| {
                    ui.fill_path(&path, btn_bg);
                    let r = tr.feather(-0.018);
                    ui.fill_rect(r, (*self.icons.info, r, ScaleType::Fit));
                });
            }
        });
        Ok(())
    }
}

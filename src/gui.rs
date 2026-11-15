//! 窓。名前とメモを聞き、測っている間の項目・回・値が届いているかを出し、ボタンとキーで進める。

use crate::i18n::{self, tr, Lang};
use crate::keys::{ChannelKeys, Input, KeySource, ScriptKeys};
use crate::session::Key;
use crate::view::{self, Level};
use crate::{measure, osc, prepare, Args, Prepared, Saved};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

/// 窓のキーの出来事を入力にする。押した出来事だけを、押しっぱなしの繰り返しを除いて数える。
pub fn key_of(ev: &egui::Event) -> Option<Key> {
    match ev {
        egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } if modifiers.is_none() => match key {
            egui::Key::Space => Some(Key::Measure),
            // HMD を着けたまま、スペースに親指を置いた左手で押せるよう、D と A を主にする。
            // 矢印キーと Enter・Backspace も同じ働きにしておく
            egui::Key::D | egui::Key::ArrowRight | egui::Key::Enter => Some(Key::Next),
            egui::Key::A | egui::Key::ArrowLeft | egui::Key::Backspace => Some(Key::Undo),
            _ => None,
        },
        _ => None,
    }
}

const GREEN: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
/// 警告の色。正常な流れ（待っている、次にすること、止めた）には使わず、記録できなかったときや始められないときだけに使う
const AMBER: Color32 = Color32::from_rgb(0xe0, 0xa0, 0x30);

enum Screen {
    Error(String),
    Setup { name: String, memo: String, prep: Box<Prepared> },
    Running,
}

struct App {
    args: Args,
    screen: Screen,
    /// OSC で受けた値の表。値が届いているかを出すために、測る前から見る
    table: Option<osc::Table>,
    last_path: PathBuf,
    out_dir: Option<PathBuf>,
    keys: Option<Sender<Input>>,
    worker: Option<JoinHandle<()>>,
    /// 測定・次へ・取り消しを最後に送った時刻。ボタンを一瞬押した色にする
    pressed: [Option<std::time::Instant>; 3],
}

pub fn run(args: Args) -> i32 {
    let auto = args.window_script.is_some();
    let last_path = args.last_file.clone().unwrap_or_else(|| crate::exe_dir().join("ft-measure-last.txt"));
    let (screen, table, out_dir) = match prepare(&args) {
        Ok(p) => {
            let table = p.table.clone();
            let out = p.out_dir.clone();
            (Screen::Setup { name: p.saved.name.clone(), memo: String::new(), prep: Box::new(p) }, Some(table), Some(out))
        }
        Err(e) => (Screen::Error(e.message), None, None),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FT Measure")
            .with_inner_size([780.0, 740.0])
            .with_min_inner_size([560.0, 480.0]),
        ..Default::default()
    };
    let r = eframe::run_native(
        "ft-measure",
        options,
        Box::new(move |cc| {
            use_japanese_font(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            view::on_change(Box::new(move || ctx.request_repaint()));
            cc.egui_ctx.all_styles_mut(|s| {
                for f in s.text_styles.values_mut() {
                    f.size *= 1.2;
                }
                s.spacing.item_spacing = egui::vec2(10.0, 8.0);
                s.spacing.button_padding = egui::vec2(12.0, 6.0);
            });
            let mut app = App { args, screen, table, last_path, out_dir, keys: None, worker: None, pressed: [None; 3] };
            if auto {
                app.start();
            }
            Ok(Box::new(app))
        }),
    );
    match r {
        Ok(()) => 0,
        Err(e) => {
            view::say(&format!("{}: {e}", tr("Cannot open the window", "窓を開けません")));
            1
        }
    }
}

/// 日本語のフォントを、英字にも使う最初のフォントにする。英字と日本語で別のフォントを使うと、行の高さがずれるため。
/// 見つからなければ、eframe が OS のフォントから足りない字を探すのに任せる。
fn use_japanese_font(ctx: &egui::Context) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(w) = std::env::var_os("WINDIR") {
        let fonts = PathBuf::from(w).join("Fonts");
        for f in ["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"] {
            candidates.push(fonts.join(f));
        }
    }
    candidates.push("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc".into());
    candidates.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into());
    let Some(data) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else { return };
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "japanese",
        egui::FontData::from_owned(data),
        vec![egui::epaint::text::InsertFontFamily {
            family: egui::FontFamily::Proportional,
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    ));
}

/// 中身が窓に収まらず `overflow` だけはみ出しているとき、画面に入る範囲で窓を縦に伸ばす。
/// 字の行の高さはフォント（Windows の游ゴシックと Mac のヒラギノ）と画面の拡大率で違い、
/// 決めた高さでは収まらないことがあるため。縮めはしない。収まらない分は今までどおりスクロールで見る。
fn fit_window(ctx: &egui::Context, overflow: f32) {
    if overflow <= 1.0 {
        return;
    }
    let (inner, monitor) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().monitor_size));
    let (Some(inner), Some(monitor)) = (inner, monitor) else { return };
    // 題の帯とタスクバーの分を残す
    let max_h = (monitor.y - 96.0).max(inner.height());
    let want = (inner.height() + overflow + 4.0).min(max_h);
    if want > inner.height() + 1.0 {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(inner.width(), want)));
    }
}

fn open_folder(dir: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

/// 字の見える部分（一番上の字の上端から、英字は並びの基準の線まで、日本語などの字は字の下端まで）の高さを測った文字列。
/// フォントによって字の上下の余白が違い（Mac のヒラギノと Windows の游ゴシックで違って見えた）、
/// 行の高さで中央に置くと字が上に寄るので、見える部分で中央に置く。下に出る部分（g や y）は数えない。
struct Ink {
    galley: std::sync::Arc<egui::Galley>,
    /// 行の上端から、見える部分の上端まで
    top: f32,
    height: f32,
}

fn ink(p: &egui::Painter, text: &str, font: egui::FontId, color: Color32) -> Ink {
    let galley = p.layout_no_wrap(text.to_string(), font, color);
    let mut top = f32::INFINITY;
    let mut base = f32::NEG_INFINITY;
    for row in &galley.rows {
        for g in &row.row.glyphs {
            if g.uv_rect.size.y > 0.0 {
                let ink_top = row.pos.y + g.pos.y + g.uv_rect.offset.y;
                top = top.min(ink_top);
                // 英字は下に出る部分（g や y）を数えず基準の線までにする。日本語の字は基準の線より少し下まで描かれ、
                // そこまで数えないと下に寄って見える（Windows の游ゴシックで見た）
                let bottom = if g.chr.is_ascii() { row.pos.y + g.pos.y } else { ink_top + g.uv_rect.size.y };
                base = base.max(bottom);
            }
        }
    }
    if !top.is_finite() || !base.is_finite() || base <= top {
        // 字の形が分からないときは、行の高さで置く
        let h = galley.size().y;
        return Ink { galley, top: 0.0, height: h };
    }
    Ink { galley, top, height: base - top }
}

impl Ink {
    /// 見える部分の中央が `center` に来るように描く。
    fn paint(self, p: &egui::Painter, center: egui::Pos2) {
        let pos = egui::pos2(center.x - self.galley.size().x / 2.0, center.y - self.height / 2.0 - self.top);
        p.galley(pos, self.galley, Color32::PLACEHOLDER);
    }
}

/// 大きいボタン。名前を中央に、キーの案内をその下に、どちらも横の中央にそろえて描く。
/// `flash` が真のときは、キーで押されたことが分かるよう押した色で描く。
fn big_button(ui: &mut egui::Ui, size: egui::Vec2, label: &str, key: Option<&str>, primary: bool, flash: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let v = ui.visuals();
    let down = flash || resp.is_pointer_button_down_on();
    let fill = match (primary, down, resp.hovered()) {
        (true, true, _) => v.selection.bg_fill.gamma_multiply(0.75),
        (true, false, true) => v.selection.bg_fill.gamma_multiply(1.15),
        (true, false, false) => v.selection.bg_fill,
        (false, true, _) => v.widgets.active.weak_bg_fill,
        (false, false, true) => v.widgets.hovered.weak_bg_fill,
        (false, false, false) => v.widgets.inactive.weak_bg_fill,
    };
    let text = if primary { Color32::WHITE } else { v.strong_text_color() };
    let weak = if primary { Color32::from_white_alpha(170) } else { v.weak_text_color() };
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(10), fill);
    let center = rect.center();
    match key {
        Some(k) => {
            // 名前とキーの名前を 1 つの塊として、上下の中央に置く
            let a = ink(p, label, egui::FontId::proportional(24.0), text);
            let b = ink(p, k, egui::FontId::proportional(13.0), weak);
            let gap = 13.0;
            let top = center.y - (a.height + gap + b.height) / 2.0;
            let (ha, hb) = (a.height, b.height);
            a.paint(p, egui::pos2(center.x, top + ha / 2.0));
            b.paint(p, egui::pos2(center.x, top + ha + gap + hb / 2.0));
        }
        None => ink(p, label, egui::FontId::proportional(22.0), text).paint(p, center),
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// 言語の切り替え。同じ幅の 2 つの区切りを、1 つの角の丸い枠に入れる。
fn lang_toggle(ui: &mut egui::Ui, cur: Lang) -> Lang {
    let cell = egui::vec2(84.0, 30.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(cell.x * 2.0, cell.y), egui::Sense::hover());
    let v = ui.visuals().clone();
    ui.painter().rect_filled(rect, egui::CornerRadius::same(8), v.extreme_bg_color);
    let mut pick = cur;
    for (i, (l, name)) in [(Lang::En, "English"), (Lang::Ja, "日本語")].into_iter().enumerate() {
        let r = egui::Rect::from_min_size(rect.min + egui::vec2(cell.x * i as f32, 0.0), cell).shrink(3.0);
        let resp = ui.interact(r, ui.scope_id().with(("lang", i)), egui::Sense::click());
        let on = l == cur;
        if on {
            ui.painter().rect_filled(r, egui::CornerRadius::same(6), v.selection.bg_fill);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, egui::CornerRadius::same(6), v.widgets.hovered.weak_bg_fill);
        }
        let color = if on { Color32::WHITE } else { v.text_color() };
        ink(ui.painter(), name, egui::FontId::proportional(15.0), color).paint(ui.painter(), r.center());
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            pick = l;
        }
    }
    pick
}

/// 入力欄の左の名前。入力欄の文字の高さに合わせて少し下げる。
fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.vertical(|ui| {
        ui.add_space(9.0);
        ui.add(egui::Label::new(text).extend());
    });
}

/// 入力欄の下の、何を入れるかの説明。
fn help(ui: &mut egui::Ui, text: &str) {
    ui.set_max_width(420.0);
    ui.add(egui::Label::new(RichText::new(text).size(14.0).weak()).wrap());
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn level_color(ui: &egui::Ui, l: Level) -> Color32 {
    match l {
        Level::Info => ui.visuals().text_color(),
        Level::Ok => GREEN,
        Level::Note => ui.visuals().strong_text_color(),
        Level::Warn => AMBER,
    }
}

impl App {
    fn start(&mut self) {
        let Screen::Setup { name, memo, .. } = &self.screen else { return };
        let (name, memo) = (name.clone(), memo.clone());
        let Screen::Setup { prep, .. } = std::mem::replace(&mut self.screen, Screen::Running) else { return };
        let (tx, rx) = mpsc::channel();
        self.keys = Some(tx);
        let args = self.args.clone();
        // 試験用: 窓を出したまま、キーを台本から読む
        let source: Box<dyn KeySource + Send> = match &args.window_script {
            Some(p) => {
                let text = std::fs::read_to_string(p).unwrap_or_default();
                match ScriptKeys::parse(&text, std::net::SocketAddr::from(([127, 0, 0, 1], args.port))) {
                    Ok(s) => Box::new(s),
                    Err(e) => {
                        view::say(&e);
                        return;
                    }
                }
            }
            None => Box::new(ChannelKeys(rx)),
        };
        self.worker = Some(std::thread::spawn(move || {
            if let Err(e) = measure(&args, *prep, name, memo, source) {
                view::log(Level::Warn, &e);
                view::set_ended(false);
            }
        }));
    }

    fn send(&mut self, k: Key) {
        self.pressed[k as usize] = Some(std::time::Instant::now());
        if let Some(tx) = &self.keys {
            let _ = tx.send(Input::Key(k));
        }
    }

    fn keys_received(&self) -> usize {
        self.table.as_ref().map(|t| t.lock().map(|m| m.len()).unwrap_or(0)).unwrap_or(0)
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("FT Measure").size(20.0).strong());
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cur = i18n::lang();
                let pick = lang_toggle(ui, cur);
                if pick != cur {
                    i18n::set_lang(pick);
                    let mut s = Saved::load(&self.last_path);
                    s.lang = Some(pick);
                    s.store(&self.last_path);
                }
            });
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui, running: bool) {
        ui.horizontal(|ui| {
            let n = self.keys_received();
            let (color, text) = if self.table.is_none() {
                (ui.visuals().weak_text_color(), tr("Not receiving", "受けていません").to_string())
            } else if n == 0 {
                (ui.visuals().weak_text_color(), format!("{} (OSC {}:{})", tr("Waiting for FT values from VRChat", "VRChat からの FT の値を待っています"), self.args.host, self.args.port))
            } else {
                (GREEN, match i18n::lang() {
                    Lang::En => format!("Receiving {n} FT parameters"),
                    Lang::Ja => format!("FT のパラメータを {n} 個受けています"),
                })
            };
            ui.label(RichText::new("●").color(color));
            ui.label(text);
            if running {
                let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if focused {
                        ui.label(RichText::new(tr("Keyboard ready", "キーボードを受けられます")).weak());
                    } else {
                        ui.label(RichText::new(tr("Click this window to use the keyboard", "キーボードを使うには、この窓をクリック")).color(AMBER));
                    }
                });
            }
        });
    }

    fn setup_ui(&mut self, ui: &mut egui::Ui) {
        let received = self.keys_received() > 0;
        let mut go = false;
        let Screen::Setup { name, memo, prep } = &mut self.screen else { return };
        ui.add_space(4.0);
        ui.heading(RichText::new(tr("Measure your face tracking values", "フェイストラッキングの値を測る")).size(26.0).strong());
        ui.label(tr(
            "Make each expression shown here and press Measure. The values VRChat sends are saved per expression.",
            "表示される表情を作って「測る」を押します。VRChat が送る値を、表情ごとに保存します。",
        ));
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new(tr("Before you start", "始める前に")).strong());
            let steps = [
                tr("In VRChat, turn on OSC (Action Menu > Options > OSC > Enabled).", "VRChat で OSC をオンにする（アクションメニュー > Options > OSC > Enabled）。"),
                tr("Change into an avatar whose face tracking outputs FT/In/ parameters.", "フェイストラッキングを FT/In/ のパラメータに出すアバターに着替える。"),
                tr("Move your face and check that the dot at the bottom turns green.", "顔を動かして、下の丸が緑になるのを確かめる。"),
            ];
            for (i, s) in steps.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).strong());
                    ui.label(*s);
                });
            }
        });
        ui.add_space(10.0);
        egui::Grid::new("setup").num_columns(2).min_row_height(38.0).spacing([14.0, 12.0]).show(ui, |ui| {
            field_label(ui, tr("Device name", "機器の名前"));
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(name).hint_text(tr("e.g. selfie, vive", "例 selfie、vive")).desired_width(420.0).margin(egui::Margin { left: 10, right: 10, top: 11, bottom: 7 }));
                help(ui, tr(
                    "The tracker you measure with. It becomes the start of the file names.",
                    "測るのに使う機器です。ファイルの名前の始めになります。",
                ));
            });
            ui.end_row();
            field_label(ui, tr("Memo", "メモ"));
            let r = ui
                .vertical(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(memo).hint_text(tr("Optional", "空でよい")).desired_width(420.0).margin(egui::Margin { left: 10, right: 10, top: 11, bottom: 7 }));
                    help(ui, tr(
                        "Conditions that can change the values, to tell sessions apart later. e.g. room lights on, camera above the monitor, VRCFT 5.3",
                        "値が変わりうる条件を書いておくと、後で測った回を見分けられます。例: 部屋の照明あり、カメラはモニタの上、VRCFT 5.3",
                    ));
                    r
                })
                .inner;
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            ui.end_row();
        });
        ui.add_space(14.0);
        let row = egui::vec2(ui.available_width(), 52.0);
        ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            if big_button(ui, egui::vec2(180.0, 52.0), tr("Start", "始める"), None, true, false) {
                go = true;
            }
            if !received {
                ui.label(RichText::new(tr("You can start before values arrive.", "値が届く前に始めてもかまいません。")).weak());
            }
        });
        ui.add_space(14.0);
        let n = prep.items.len();
        // 一覧のファイルは、長いパスで行が読みにくくならないよう、ファイルの名前だけを出す
        let src = if prep.items_label == "(built-in)" {
            tr("built-in", "組み込み").to_string()
        } else {
            Path::new(&prep.items_label).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| prep.items_label.clone())
        };
        ui.label(RichText::new(match i18n::lang() {
            Lang::En => format!("{n} items ({src}). Each pose is measured 3 times."),
            Lang::Ja => format!("{n} 項目（{src}）。表情は 3 回ずつ測ります。"),
        }).weak());
        ui.add(egui::Label::new(RichText::new(format!("{}: {}", tr("Saved to", "保存先"), prep.out_dir.display())).weak()).truncate());
        if go {
            self.start();
        }
    }

    fn running_ui(&mut self, ui: &mut egui::Ui) {
        let v = view::snapshot();
        let p = &v.progress;

        if v.ended.is_none() {
            // 測っている間は、どの部品にも focus を置かず、キーを全部この画面で受ける
            if let Some(id) = ui.ctx().memory(|m| m.focused()) {
                ui.ctx().memory_mut(|m| m.surrender_focus(id));
            }
            let keys: Vec<Key> = ui.ctx().input(|i| i.events.iter().filter_map(key_of).collect());
            for k in keys {
                self.send(k);
            }
        }

        if let Some(finished) = v.ended {
            self.ended_ui(ui, finished, &v);
            return;
        }

        // 進み具合
        if p.total > 0 {
            ui.label(RichText::new(match i18n::lang() {
                Lang::En => format!("Item {} of {}", p.no, p.total),
                Lang::Ja => format!("{} / {} 項目", p.no, p.total),
            }).weak());
            ui.add(egui::ProgressBar::new((p.no.saturating_sub(1)) as f32 / p.total as f32).desired_height(6.0));
        }
        ui.add_space(8.0);

        // 今の項目
        card(ui, |ui| {
            ui.label(RichText::new(if p.title.is_empty() { "…" } else { &p.title }).size(28.0).strong());
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if !p.step.is_empty() {
                    ui.label(RichText::new(&p.step).size(22.0).color(ui.visuals().selection.stroke.color));
                }
                if !p.hint.is_empty() {
                    ui.label(RichText::new(&p.hint).size(18.0).color(ui.visuals().strong_text_color()));
                }
            });
        });

        // 最後の出来事
        let last = v.log.iter().rev().find(|(l, _)| *l != Level::Info);
        ui.add_space(4.0);
        match last {
            Some((l, m)) => ui.label(RichText::new(m).color(level_color(ui, *l))),
            None => ui.label(RichText::new(tr(
                "Make the expression, hold it, and press Measure.",
                "表情を作って保ったまま、「測る」を押します。",
            )).weak()),
        };
        ui.add_space(8.0);

        // ボタン
        let gap = 14.0;
        let w = (ui.available_width() - 2.0 * gap) / 3.0;
        let flash = |i: usize| self.pressed[i].is_some_and(|t| t.elapsed() < Duration::from_millis(180));
        let (f0, f1, f2) = (flash(0), flash(1), flash(2));
        if f0 || f1 || f2 {
            ui.ctx().request_repaint_after(Duration::from_millis(60));
        }
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let size = egui::vec2(w, 76.0);
            if big_button(ui, size, tr("Measure", "測る"), Some(tr("Space", "スペース")), true, f0) {
                clicked = Some(Key::Measure);
            }
            if big_button(ui, size, tr("Next", "次へ"), Some(tr("D key", "D キー")), false, f1) {
                clicked = Some(Key::Next);
            }
            if big_button(ui, size, tr("Undo", "取り消し"), Some(tr("A key", "A キー")), false, f2) {
                clicked = Some(Key::Undo);
            }
        });
        if let Some(k) = clicked {
            self.send(k);
        }

        ui.add_space(6.0);
        egui::CollapsingHeader::new(tr("Activity", "記録の経過")).default_open(false).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(160.0).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
                for (l, line) in &v.log {
                    ui.label(RichText::new(line).monospace().color(level_color(ui, *l)));
                }
            });
        });
        if let Some(s) = &v.summary_path {
            ui.add(egui::Label::new(RichText::new(format!("{}: {}", tr("Saving to", "保存先"), s.display())).weak().small()).truncate());
        }
    }

    fn ended_ui(&self, ui: &mut egui::Ui, finished: bool, v: &view::View) {
        ui.add_space(8.0);
        if finished {
            ui.heading(RichText::new(tr("All items measured", "全部の項目を測りました")).size(26.0).color(GREEN));
        } else {
            ui.heading(RichText::new(tr("Stopped. The items measured so far are saved", "止めました。それまでに測った分は保存してあります")).size(22.0).color(ui.visuals().strong_text_color()));
        }
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.label(RichText::new(tr("Saved files", "保存したファイル")).strong());
            if let Some(dir) = v.summary_path.as_ref().and_then(|p| p.parent()) {
                ui.add(egui::Label::new(RichText::new(dir.display().to_string()).weak()).truncate());
            }
            for p in [&v.summary_path, &v.raw_path].into_iter().flatten() {
                if let Some(n) = p.file_name() {
                    ui.label(RichText::new(format!("• {}", n.to_string_lossy())).monospace());
                }
            }
            if v.summary_path.is_none() {
                for (l, m) in v.log.iter().filter(|(l, _)| *l == Level::Warn) {
                    ui.label(RichText::new(m).color(level_color(ui, *l)));
                }
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let dir = v.summary_path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(|| self.out_dir.clone());
            if let Some(d) = dir {
                if big_button(ui, egui::vec2(190.0, 48.0), tr("Open folder", "フォルダを開く"), None, true, false) {
                    open_folder(&d);
                }
            }
            if big_button(ui, egui::vec2(140.0, 48.0), tr("Close", "閉じる"), None, false, false) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let running = matches!(self.screen, Screen::Running) && view::snapshot().ended.is_none();
        let bar = |ui: &egui::Ui| {
            egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(24, 12))
        };
        egui::Panel::top("top").frame(bar(ui)).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom").frame(bar(ui)).show(ui, |ui| self.status_bar(ui, running));
        let body = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(24, 20));
        egui::CentralPanel::default().frame(body).show(ui, |ui| {
            let out = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &self.screen {
                Screen::Error(e) => {
                    let e = e.clone();
                    ui.add_space(8.0);
                    ui.heading(RichText::new(tr("Cannot start", "始められません")).size(26.0).color(AMBER));
                    ui.add_space(8.0);
                    ui.label(e);
                    ui.add_space(12.0);
                    if ui.button(tr("Close", "閉じる")).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                Screen::Setup { .. } => self.setup_ui(ui),
                Screen::Running => self.running_ui(ui),
            });
            fit_window(ui.ctx(), out.content_size.y - out.inner_rect.height());
        });
        // 値が届いているかを見直すため、何も起きなくても時々描き直す
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 送り口を落とすと、測るスレッドは「終わる」と受けて、書き出してから終わる
        self.keys = None;
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: egui::Key, pressed: bool, repeat: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed, repeat, modifiers }
    }

    #[test]
    fn gui_keys_map_and_ignore_repeat_release_and_modifiers() {
        let n = egui::Modifiers::NONE;
        assert_eq!(key_of(&ev(egui::Key::Space, true, false, n)), Some(Key::Measure));
        assert_eq!(key_of(&ev(egui::Key::Enter, true, false, n)), Some(Key::Next));
        assert_eq!(key_of(&ev(egui::Key::Backspace, true, false, n)), Some(Key::Undo));
        assert_eq!(key_of(&ev(egui::Key::ArrowRight, true, false, n)), Some(Key::Next));
        assert_eq!(key_of(&ev(egui::Key::ArrowLeft, true, false, n)), Some(Key::Undo));
        assert_eq!(key_of(&ev(egui::Key::Space, true, true, n)), None, "押しっぱなしの繰り返し");
        assert_eq!(key_of(&ev(egui::Key::Space, false, false, n)), None, "離した出来事");
        assert_eq!(key_of(&ev(egui::Key::Space, true, false, egui::Modifiers::CTRL)), None);
        assert_eq!(key_of(&ev(egui::Key::D, true, false, n)), Some(Key::Next));
        assert_eq!(key_of(&ev(egui::Key::A, true, false, n)), Some(Key::Undo));
        assert_eq!(key_of(&ev(egui::Key::S, true, false, n)), None);
    }
}

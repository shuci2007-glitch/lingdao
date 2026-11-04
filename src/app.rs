//! 灵动岛主体：状态、悬停形变、切页、顶栏与公共层。与平台无关——
//! Windows 外壳（`main.rs`）和无窗口录制器（`daozai-record`）共用同一份代码。
//!
//! 时间一律来自 `Env::t`（秒），不直接读系统时钟，所以录屏可逐帧复现。

use crate::extras::{Alert, PetState};
use crate::anim::{self, lerp, smoothstep, Spring};
use crate::bar_layout::{self as bar, Layout};
use crate::calendar;
use crate::glass;
use crate::lyrics;
use crate::mail;
use crate::model;
use crate::paint::{self, Card, H};
use crate::sfx;
use crate::activity::{Activity, Kind};
use crate::shape;
use crate::smtc::{NowPlaying, Transport};
use crate::theme::{self, size, W};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;

// ---------- 几何（逻辑点；原版 125% 截图 ÷ 1.25） ----------
pub const PW: f32 = 419.0; // 展开面板宽
pub const PH: f32 = 140.0; // 展开面板高
pub(crate) const CALENDAR_PAGE: usize = 7;
pub(crate) const CALENDAR_H: f32 = 216.0;
pub const BAR_W: f32 = 163.0; // 收起药丸宽
pub const BAR_H: f32 = 30.0; // 收起药丸高
pub(crate) const HUD_W: f32 = 260.0;
pub(crate) const LYRIC_BAR_W: f32 = 300.0;
pub(crate) const HUD_TRACK_X: f32 = 48.0;
pub(crate) const HUD_TRACK_W: f32 = 150.0;
pub const BAR_W_MIN: f32 = 44.0; // 极简小条
pub const BAR_H_MIN: f32 = 4.5;
/// 空闲小药丸：比手动极简的细线更饱满，像真机灵动岛收起时那颗黑丸。
pub const IDLE_PILL_W: f32 = 116.0;
pub const IDLE_PILL_H: f32 = 13.0;
/// 「水滴」形态收束到底时的尺寸：一颗小水珠贴在屏顶中央，像融入顶部。
pub const IDLE_DROP_W: f32 = 14.0;
pub const IDLE_DROP_H: f32 = 11.0;
/// 灵动岛当前应呈现的形态（单一判定入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Present { Bar, Pill, Hidden }
pub const FLARE: f32 = 17.6; // 顶部外翻肩宽
pub const FLARE_H: f32 = 16.0; // 外翻收敛高度
pub const RAD: f32 = 33.0; // 底部圆角
pub const WIN_W: f32 = 640.0; // 窗口（透明画布）
pub const CANVAS_W: f32 = WIN_W + 640.0; // 为右侧通知预留透明画布，原岛仍按 WIN_W 定位。
pub const WIN_H: f32 = 270.0;
pub(crate) const PAGE_NAMES: [&str; 8] = ["音乐", "专注", "饮水", "置物架", "待办", "邮箱", "AI 额度", "日程"];

// ---------- 动效参数 ----------
/// 悬停意图缓冲：光标停留这么久才真正展开（期间药丸轻微胀大作为反馈），防误触。
const HOVER_INTENT: f64 = 0.20;
const MINIMAL_HOVER_INTENT: f64 = 0.40;
/// 移开后等待多久开始收起；这段时间内回来不会收。
/// 鼠标离开岛体后，等待多久才自动收起（秒）。
const SWELL: (f32, f32) = (1.075, 1.13);
// 展开：欠阻尼，末段约 5% 过冲后回落；高度略软于宽度，像「垂落」下来。
const OPEN_W: (f32, f32) = (330.0, 0.70);
const OPEN_H: (f32, f32) = (290.0, 0.74);
// 收起：临界阻尼，约 0.18 s 干净收回，无回弹。
const CLOSE: (f32, f32) = (620.0, 1.0);
const SWELL_K: (f32, f32) = (420.0, 0.8);
const PAGE_SLIDE: f32 = 46.0;

pub struct Options {
    /// 是否联网查歌词（录制/测试时关掉）。
    pub fetch_lyrics: bool,
    /// 吉祥物在多久无操作后出现（秒）。
    pub pet_idle: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self { fetch_lyrics: true, pet_idle: 45.0 }
    }
}

/// 每帧由外壳提供的外部世界。
#[derive(Clone, Default)]
pub struct Env {
    pub t: f64,
    /// `HH:MM:SS`
    pub clock: String,
    /// `YYYY-MM-DD`
    pub today: String,
    /// 顶栏日期，例如 `10月28日 周三`
    pub date: String,
    pub battery: Option<u8>,
    pub charging: bool,
    /// 互联网连接状态；未知时不触发表情提醒。
    pub network: Option<bool>,
    /// 光标位置（与 `frame` 传入的 `win` 同一坐标系）；不在窗口附近时为 None。
    pub cursor: Option<Pos2>,
    pub idle_seconds: Option<f64>,
    /// 前台浏览器的顶部是否与岛重叠；灵岛自身窗口或暂不可读时为 None。
    pub browser_at_top: Option<bool>,
    pub media: NowPlaying,
    /// 系统主音量（0..100, 是否静音）；v0.5.4 音量条与系统双向同步。
    pub volume: Option<(u32, bool)>,
    /// 已连接的蓝牙音频设备（名称 + 可选电量）。
    pub bt: Vec<crate::bt::Device>,
    /// 蓝牙服务是否已经完成首次扫描。
    pub bt_ready: bool,
}

impl Env {
    pub fn hour(&self) -> usize {
        self.clock.get(0..2).and_then(|s| s.parse().ok()).unwrap_or(12)
    }
    pub fn minute(&self) -> u32 {
        self.clock.get(3..5).and_then(|s| s.parse().ok()).unwrap_or(0)
    }
}

/// 交给外壳执行的副作用。
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Media(Transport),
    Open(PathBuf),
    OpenUrl(String),
    /// 需要键盘输入：外壳应让窗口可激活并取得焦点。
    Focus,
    Reveal(PathBuf),
    Copy(String),
    Notify(String, String),
    RequestNotifications,
    DisconnectNotifications,
    ToggleAutostart,
    AddFiles,
    DragFile(PathBuf),
    QuotaRefresh,
    QuotaConnected,
    QuotaChooseFile,
    /// 把系统主音量设为 0..100（v0.5.4 音量条）。
    SetVolume(u32),
    /// 切换系统静音。
    ToggleMute,
    /// v0.5.5：账号清单变了，让外壳重新同步收信线程。
    MailSync,
    /// 测试某个邮箱的连接（授权码只在内存里传给工作线程）。
    MailTest(mail::Account, String),
    /// v0.5.6：用临时密码试连日历，成功后才允许落盘。
    CalendarTest(calendar::Config, String),
    /// 让外壳立即同步一次日历。
    CalendarSync,
    CalendarMonth(chrono::NaiveDate),
    /// 断开并删除日历配置（凭据由界面删除）。
    CalendarForget,
}

pub struct FrameOut {
    /// 原生窗口应保留的可见区域（相对 `win` 左上角）。
    pub region: Rect,
    /// 鼠标命中区（相对 `win` 左上角），区域外点击穿透到下层窗口。
    pub hit: Rect,
    pub notice_hit: Option<Rect>,
    /// 文件拖入感应区；独立于普通鼠标命中区。
    pub file_drop_zone: Option<Rect>,
    /// 展开状态立即生效，不等待展开动画结束。
    pub expanded: bool,
    pub editing: bool,
    pub animating: bool,
    pub repaint_after: std::time::Duration,
    pub cmds: Vec<Cmd>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Focus,
    Rest,
}

impl Phase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Phase::Focus => "专注时间",
            Phase::Rest => "休息",
        }
    }
}

pub(crate) struct Pomo {
    pub phase: Phase,
    pub remaining: f32,
    pub running: bool,
}

#[derive(Default)]
pub(crate) struct LyricsState {
    pub key: String,
    pub loading: bool,
    pub error: Option<lyrics::FetchError>,
    pub retry_at: Option<f64>,
    pub lyrics: Option<lyrics::Lyrics>,
    pub rx: Option<std::sync::mpsc::Receiver<Result<(lyrics::Lyrics,bool), lyrics::FetchError>>>,
    pub native_rx: Option<std::sync::mpsc::Receiver<Option<lyrics::Lyrics>>>,
}

/// v0.5.5：邮件面板与设置表单的界面状态。正文只在内存里过一次，不落盘。
#[derive(Default)]
pub(crate) struct MailState {
    pub accounts: Vec<mail::Account>,
    /// 每个账号的连接状态，键是邮箱小写。
    pub status: HashMap<String, mail::Status>,
    /// 最近邮件，新的在前，最多 60 封。
    pub recent: Vec<mail::Item>,
    /// 设置页里「测试连接」的结果。
    pub note: String,
    pub testing: Option<String>,
    pub form: MailForm,
    /// 面板里点开的邮件（账号 + uid，避免两个账号 uid 撞车）。
    pub open_uid: Option<(String, u32)>,
    pub reading: Option<mail::Item>,
    pub reader_focus: bool,
    /// 已经提醒过的会议，键是 `UID + DTSTART`（邮件和日历两条来源共用）。
    pub fired: std::collections::HashSet<String>,
}

#[derive(Default)]
pub(crate) struct MailForm {
    pub provider: mail::Provider,
    pub email: String,
    pub password: String,
    pub show: bool,
    /// 自定义 IMAP 的服务器地址与端口（provider 选「其他邮箱」时才用）。
    pub server: String,
    pub port: String,
}

impl MailState {
    /// 最近一封带验证码的邮件。
    pub fn code_item(&self) -> Option<&mail::Item> {self.recent.iter().find(|m|m.code.is_some())}
    /// 从岛内列表删掉一封邮件：只动本地列表，不删服务器上的邮件。
    pub fn dismiss(&mut self, account: &str, uid: u32) {
        self.recent.retain(|m| m.uid != uid || !m.account.eq_ignore_ascii_case(account));
        if self.open_uid.as_ref().is_some_and(|(a, u)| *u == uid && a.eq_ignore_ascii_case(account)) {
            self.open_uid = None;
        }
        if self.reading.as_ref().is_some_and(|m| m.uid == uid && m.account.eq_ignore_ascii_case(account)) {
            self.reading = None;
        }
    }
    /// 清空岛内列表（含已打开的详情），服务器上的邮件不受影响。
    pub fn clear(&mut self) {
        self.recent.clear();
        self.open_uid = None;
        self.reading = None;
    }
    pub fn status_of(&self, email: &str) -> mail::Status {
        self.status
            .get(&email.to_ascii_lowercase())
            .cloned()
            .unwrap_or(mail::Status::Idle)
    }
}

/// v0.5.6：日程（CalDAV）的界面状态。密码只在表单里暂存，保存后立刻清掉。
#[derive(Default)]
pub(crate) struct CalendarState {
    pub local: Vec<crate::local_calendar::Entry>,
    pub local_error: String,
    pub draft: Option<crate::local_calendar::Draft>,
    pub feeds: Vec<CalendarFeed>,
    pub config: Option<calendar::Config>,
    /// 未来 24 小时的日程（提醒也读这里）。
    pub events: Vec<mail::Meeting>,
    pub view: crate::calendar_ui::State,
    pub connected: bool,
    pub last_sync: String,
    /// 连续失败次数；>= 2 才在设置页显示红字。
    pub failures: u32,
    pub error: String,
    pub testing: bool,
    /// 设置表单里的提示（成功/失败/粘贴拆分的说明）。
    pub note: String,
    /// `note` 是不是错误提示（界面显示成红字）。
    pub note_error: bool,
    pub form: CalendarForm,
    /// 「测试并保存」用的临时密码，测试成功才落盘。
    pub test_password: String,
}

pub(crate) struct CalendarFeed {
    pub config: calendar::Config,
    pub events: Vec<mail::Meeting>,
    pub month: crate::calendar_ui::State,
    pub connected: bool,
    pub last_sync: String,
    pub failures: u32,
    pub error: String,
}

impl CalendarFeed {
    fn new(config: calendar::Config) -> Self {
        Self { config, events: vec![], month: Default::default(), connected: false,
            last_sync: String::new(), failures: 0, error: String::new() }
    }
}

#[derive(Default)]
pub(crate) struct CalendarForm {
    pub provider: calendar::Provider,
    pub username: String,
    pub password: String,
    pub show: bool,
    pub guide_open: bool,
    /// 自定义服务器地址（自建/反代时填，覆盖默认 host；会做同源校验）。
    pub server: String,
}

impl CalendarState {
    pub fn load() -> Self {
        let config = calendar::load_config();
        let form = CalendarForm { provider: calendar::selected_provider(),
            username: config.as_ref().map(|cfg| cfg.username.clone()).unwrap_or_default(),
            server: config.as_ref().and_then(|cfg| cfg.server.clone()).unwrap_or_default(), ..Default::default() };
        let feeds = calendar::load_configs().into_iter().map(CalendarFeed::new).collect();
        let (local,local_error)=match crate::local_calendar::load(){Ok(events)=>(events,String::new()),Err(error)=>(vec![],error)};
        Self { config, form, feeds, local, local_error, ..Default::default() }
    }

}

/// 把设置里的主色文本落到全局。空串或非法值都回落默认蓝。
pub(crate) fn apply_accent(text: &str) {
    let color = if text.trim().is_empty() {
        theme::BLUE
    } else {
        theme::parse_hex(text).unwrap_or(theme::BLUE)
    };
    theme::set_accent(color);
}

pub struct App {
    pub opts: Options,
    pub settings: model::Settings,
    pub(crate) todos: model::TodoList,
    pub(crate) water: model::Water,
    pub(crate) shelf: model::Shelf,

    // 形变
    w: Spring,
    h: Spring,
    pub(crate) open: bool,
    hover_since: Option<f64>,
    leave_since: Option<f64>,
    /// 前台切换导致的强制收起：光标离开前不再自动展开。
    pub hover_lock: bool,
    pub(crate) reveal: f32,

    // 切页
    pub(crate) page: usize,
    page_from: usize,
    page_dir: f32,
    page_t0: f64,
    slide: Spring,
    dots: [Spring; 8],
    wheel_delta: f32,
    wheel_at: f64,
    wheel_page_at: f64,

    // 拖入
    pub(crate) drag_hot: bool,
    pub(crate) drag_a: f32,
    drag_saved: Option<usize>,

    // 置物架
    pub(crate) shelf_scroll: Spring,
    pub(crate) shelf_first: usize,
    pub(crate) shelf_x: HashMap<PathBuf, Spring>,
    pub(crate) shelf_born: HashMap<PathBuf, f64>,
    pub(crate) icons: crate::icons::Cache,
    pub(crate) confirm_clear: Option<f64>,

    // 饮水
    pub(crate) water_ring: Spring,
    pub(crate) water_ml: f32,
    pub(crate) water_view: usize,
    pub(crate) water_view_t: Spring,
    pub(crate) remind_t: Spring,
    pub(crate) last_reminder: f64,

    // 专注
    pub(crate) pomo: Pomo,
    /// 专注完成记录（落盘，用于今日/本周统计）。
    pub(crate) pomo_log: crate::model::PomoLog,
    pub(crate) ring_shown: Spring,
    pub(crate) reset_spin: Spring,

    // 待办
    pub(crate) adding: bool,
    pub(crate) editing: Option<usize>,
    pub(crate) buf: String,
    pub(crate) todo_input_focused: bool,
    pub(crate) todo_pop: Spring,

    // 音乐
    pub(crate) ly: LyricsState,
    pub(crate) show_lyrics: bool,
    island_lyric_layout: Option<std::sync::Arc<egui::Galley>>,
    media_anchor: (String, f64, f64, bool),
    pub(crate) media: NowPlaying,
    pub(crate) music_artwork: Option<(std::sync::Arc<egui::ColorImage>,egui::TextureHandle)>,

    pub(crate) forced_open_until:f64,
    /// 久坐提醒的计时起点（秒，0 表示未开始）。
    pub(crate) sit_since: f64,
    pub settings_open:bool,
    browser_at_top:bool,
    browser_minimal_active:bool,
    pub(crate) settings_focus_requested:bool,
    pub(crate) settings_tab:usize,
    pub(crate) settings_category:usize,
    pub(crate) notification_apps:model::NotificationApps,
    /// 已连接的显示屏列表（设置页「显示屏」下拉用）。
    pub displays: Vec<crate::display_layout::Display>,
    /// 指定应用自动避让的「添加应用」选择器状态。
    pub(crate) avoidance: crate::avoidance::Picker,
    /// 验证码遮罩：记录首次见到时间与临时显示截止时间。
    pub(crate) privacy: crate::mail_privacy::Privacy,
    pub(crate) subpage:Option<usize>,
    pub(crate) task_dialog:Option<crate::extras::TaskDraft>,
    pub listener_status: String,
    pub(crate) notification_bell_until: f64,
    pub(crate) notification_bell_at: f64,
    pub(crate) notice: Option<Alert>,
    pub(crate) hud_notice: Option<Alert>,
    hud_alpha: f32,
    notice_width: Spring,
    /// 分裂小胶囊与主岛之间的间隙（0=合并，8=分裂），弹簧驱动液态开合。
    side_gap: Spring,
    /// 本帧进入分裂小胶囊的活动（统一活动卡协议）。
    pub(crate) side_slot: Option<crate::activity::Activity>,
    /// 最近一次收到的邮件验证码与时间，10 分钟内可在分裂胶囊中一键读取。
    pub(crate) code_seen: Option<(String, f64)>,
    notice_details: bool,
    pub(crate) alerts: std::collections::VecDeque<Alert>,
    pub(crate) history: std::collections::VecDeque<Alert>,
    pub(crate) pending: Vec<Cmd>,
    pub(crate) todo_first: usize,
    pub(crate) pet: crate::pet::Pet,
    pub(crate) last_drink: Option<(u32, usize)>,
    // 其它
    toast: Option<(String, f64)>,
    toast_below: bool,
    last_input: f64,
    system_idle: Option<f64>,
    last_cursor: Option<Pos2>,
    pet_a: f32,
    last_t: Option<f64>,
    pub(crate) t: f64,
    pub(crate) dt: f32,
    pub(crate) today: String,
    pub(crate) current_hour: usize,
    pub(crate) current_minute: u32,
    pub(crate) cursor_local: Option<Pos2>,
    started: bool,
    /// v0.5.4：音量条展示值（拖动时先用本地值，随后跟上系统回报）。
    pub(crate) volume_ui: Option<(u32, bool)>,
    /// 拖动/滚轮刚改过音量，这段时间内系统回声不再单独弹一条。
    pub(crate) volume_quiet_until: f64,
    /// 音量条填充的缓动量（0..1），让数值变化有「量感」。
    pub(crate) volume_a: f32,
    /// 上一次看到的蓝牙设备，用来只在连接/断开时提示一次。
    pub(crate) bt_shown: Vec<crate::bt::Device>,
    bt_initialized: bool,
    /// v0.5.5：邮件账号、最近邮件与设置表单。
    pub(crate) mail: MailState,
    /// v0.5.6：日历（CalDAV）配置、未来 24 小时事件与设置表单。
    pub(crate) calendar: CalendarState,
    pub(crate) quota: crate::quota_ui::State,
    pub(crate) updates: crate::updates::State,
    pub(crate) resident: bar::Motion,
    pub(crate) resident_power: (Option<u8>,bool),
    /// 已经生效的主色文本，用来发现设置变化。
    accent_applied: String,

    // 灵动岛：悬停 peek / 长按快捷操作 / 岛史 / 夜间 / 会议勿扰
    /// 悬停预览弹簧（0 = 收起，1 = 放大预览）。
    peek: Spring,
    peek_since: Option<f64>,
    /// 按住胶囊的起始时间（用于长按触发快捷操作）。
    press_since: Option<f64>,
    /// 长按后弹出的快捷操作所属活动类型；None 表示不显示。
    quick_menu: Option<crate::activity::Kind>,
    /// 屏上放不下的次级活动（>2 个时）进入岛史，点小圆可翻看。
    island_history: std::collections::VecDeque<crate::activity::Activity>,
    /// 岛史面板是否展开。
    history_open: bool,
    /// 当前夜间系数（0 白天，1 深夜），由本地时间平滑得出。
    night: f32,
    /// 本帧是否处于会议 / 投屏勿扰。
    dnd_now: bool,
    /// 会议检测是有开销的进程枚举，按 ~1.5s 缓存一次（环境变量时间）。
    dnd_cache: bool,
    dnd_checked_at: f64,
    /// 本帧光标是否贴近屏顶（用于空闲「完全隐入」时的温柔冒头）。
    near_top_now: bool,
    /// 在设置里切换空闲形态后，强制预览到该时刻，确保立刻可见。
    idle_preview_until: f64,
    /// 「水滴」形态：收成小药丸后继续像水滴一样融入顶部的进度（0=药丸，1=完全融入）。
    idle_drop: f32,
    /// 已播报过的活动签名，用来只在「新活动出现」时响一声。
    sound_sig: Vec<String>,
    sound_at: f64,
    /// 上一帧的展开状态，用于检测「展开」瞬间的啵声。
    was_open: bool,
}

impl App {
    pub fn new(ctx: &egui::Context, opts: Options) -> Self {
        theme::load_fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        let settings = model::Settings::load();
        let settings_accent = settings.accent.clone();
        // 主色和缩放要在装样式之前落地，否则第一帧会用默认蓝画一遍。
        apply_accent(&settings.accent);
        ctx.set_zoom_factor(settings.ui_zoom);
        ctx.set_theme(egui::ThemePreference::Dark);
        ctx.set_visuals(egui::Visuals::dark());
        ctx.style_mut(|s| {
            s.interaction.selectable_labels = false;
            s.visuals.text_cursor.stroke = egui::Stroke::new(1.5_f32, theme::accent());
            s.visuals.selection.bg_fill = theme::alpha_accent(0.45);
        });
        let (bw, bh) = if settings.minimal { (BAR_W_MIN, BAR_H_MIN) } else { (BAR_W, BAR_H) };
        let focus_seconds=settings.focus_minutes as f32*60.0;
        let initial_lyrics=settings.lyrics_enabled;
        let initial_page=settings.page_order.iter().copied().find(|page|settings.visible_pages[*page]).unwrap_or(0);
        Self {
            opts,
            settings,
            todos: model::TodoList::load(),
            water: model::Water::default(),
            shelf: model::Shelf::load(),
            w: Spring::new(bw, CLOSE.0, CLOSE.1),
            h: Spring::new(bh, CLOSE.0, CLOSE.1),
            open: false,
            hover_since: None,
            leave_since: None,
            hover_lock: false,
            reveal: 0.0,
            page: initial_page,
            page_from: initial_page,
            page_dir: 1.0,
            page_t0: -10.0,
            slide: Spring::new(0.0, 260.0, 0.92),
            dots: std::array::from_fn(|i| Spring::new(if i == initial_page { 16.0 } else { 4.0 }, 380.0, 0.86)),
            wheel_delta: 0.0,
            wheel_at: -1.0,
            wheel_page_at: -1.0,
            drag_hot: false,
            drag_a: 0.0,
            drag_saved: None,
            shelf_scroll: Spring::new(0.0, 300.0, 0.9),
            shelf_first: 0,
            shelf_x: HashMap::new(),
            shelf_born: HashMap::new(),
            icons: Default::default(),
            confirm_clear: None,
            water_ring: Spring::new(0.0, 120.0, 0.9),
            water_ml: 0.0,
            water_view: 0,
            water_view_t: Spring::new(0.0, 200.0, 0.9),
            remind_t: Spring::new(1.0, 400.0, 0.8),
            last_reminder: 0.0,
            pomo: Pomo { phase: Phase::Focus, remaining: focus_seconds, running: false },
            pomo_log: crate::model::PomoLog::load(),
            ring_shown: Spring::new(1.0, 160.0, 0.95),
            reset_spin: Spring::new(0.0, 160.0, 0.82),
            adding: false,
            todo_input_focused: false,
            editing: None,
            buf: String::new(),
            todo_pop: Spring::new(0.0, 500.0, 0.45),
            ly: LyricsState::default(),
            show_lyrics: initial_lyrics,
            island_lyric_layout: None,
            media_anchor: (String::new(), 0.0, 0.0, false),
            media: NowPlaying::default(),
            music_artwork: None,
            forced_open_until:0.0, sit_since:0.0, settings_open:false, browser_at_top:false, browser_minimal_active:false, settings_focus_requested:false, settings_tab:0, settings_category:0, notification_apps:model::NotificationApps::load(), displays:Vec::new(), avoidance:Default::default(), privacy:Default::default(), subpage:None, task_dialog:None,
            listener_status: "尚未连接".into(),
            notification_bell_until: 0.0, notification_bell_at: -10.0, notice: None, hud_notice: None, hud_alpha: 0.0, notice_width:Spring::new(0.0,CLOSE.0,CLOSE.1), side_gap:Spring::new(0.0,380.0,0.72), side_slot:None, code_seen:None, notice_details:false, alerts: Default::default(), history: Default::default(), pending: vec![],
            todo_first: 0, pet: Default::default(), last_drink: None,
            toast: None, toast_below: false,
            last_input: 0.0, system_idle: None,
            last_cursor: None,
            pet_a: 0.0,
            last_t: None,
            t: 0.0,
            dt: 0.0,
            today: String::new(), current_hour: 0, current_minute: 0,
            cursor_local: None,
            started: false,
            volume_ui: None,
            volume_quiet_until: 0.0,
            volume_a: 0.0,
            bt_shown: Vec::new(),
            bt_initialized: false,
            mail: MailState { accounts: mail::load_accounts(), ..Default::default() },
            calendar: CalendarState::load(),
            quota: crate::quota_ui::State::load(),
            updates: Default::default(),
            resident: Default::default(), resident_power:(None,false),
            accent_applied: settings_accent.clone(),
            peek: Spring::new(0.0, 320.0, 0.78),
            peek_since: None,
            press_since: None,
            quick_menu: None,
            island_history: Default::default(),
            history_open: false,
            night: 0.0,
            dnd_now: false,
            near_top_now: false,
            idle_preview_until: 0.0,
            idle_drop: 0.0,
            dnd_cache: false,
            dnd_checked_at: 0.0,
            sound_sig: Vec::new(),
            sound_at: 0.0,
            was_open: false,
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.t));
        self.toast_below = false;
    }

    pub(crate) fn code_copied(&mut self, code:&str) {
        self.say(format!("验证码 {code} 已复制"));
        self.toast_below = true;
    }

    /// 收信服务推来的事件；历史邮件静默，新邮件进入通知队列。
    pub fn mail_event(&mut self, ev: mail::Event) {
        let watching=|email:&str|self.settings.mail_enabled && self.mail.accounts.iter().any(|a|a.enabled && a.email.eq_ignore_ascii_case(email));
        match ev {
            mail::Event::Status { account, status } => {
                if !watching(&account){return;}
                self.mail.status.insert(account.to_ascii_lowercase(), status);
            }
            mail::Event::Mail { item, new } => {
                if !watching(&item.account){return;}
                self.mail
                    .recent
                    .retain(|m| !(m.account.eq_ignore_ascii_case(&item.account) && m.uid == item.uid));
                let incoming_code = item.code.clone();
                self.mail.recent.insert(0, item);
                self.mail.recent.truncate(60);
                if let Some(code) = incoming_code {
                    self.code_seen = Some((code, self.t));
                }
                if new && self.settings.mail_enabled {
                    let item=&self.mail.recent[0];
                    self.alert("新邮件",format!("{} · {}",item.from,item.subject),5,false);
                    self.notification_bell_at = self.t;
                    self.notification_bell_until = self.t + 6.0;
                }
            }
            mail::Event::Test { account, ok, note } => {
                self.mail.testing = None;
                let detail = if ok { "连接成功".to_string() } else { note };
                let detail = if detail.chars().count() > 120 {
                    format!("{}…", detail.chars().take(120).collect::<String>())
                } else {
                    detail
                };
                self.mail.note = format!("{account}：{detail}");
                self.say(if ok {
                    format!("{account} 连接成功")
                } else {
                    format!("{account} 连接失败")
                });
            }
        }
    }

    /// 设置窗口第 8 个标签页：邮件。
    pub fn open_mail_settings(&mut self) {
        self.settings_tab = 7;
        self.open_settings();
    }

    /// v0.5.6：日历服务推来的事件。失败不弹窗，只累计次数（连续两次才显示红字）。
    pub fn calendar_event(&mut self, ev: calendar::Event) {
        match ev {
            calendar::Event::Test { ok, message, count, config } => {
                self.calendar.testing = false;
                if !ok {
                    self.calendar.note = message;
                    self.calendar.note_error = true;
                    return;
                }
                let Some(cfg) = config else {
                    self.calendar.note = "服务器没有返回日历地址".into();
                    self.calendar.note_error = true;
                    return;
                };
                if let Err(e) = calendar::save_connection(&cfg, &self.calendar.test_password, self.calendar.config.as_ref()) {
                    self.calendar.note = e;
                    self.calendar.note_error = true;
                    return;
                }
                self.calendar.note = format!("已连接 · 未来 24 小时 {count} 个日程");
                self.calendar.note_error = false;
                if self.calendar.config.as_ref()!=Some(&cfg) {
                    self.calendar.events.clear();
                    self.calendar.view.events.clear();
                    self.calendar.view.loaded=None;
                    self.calendar.view.loading=false;
                    self.calendar.view.error.clear();
                }
                if !self.calendar.feeds.iter().any(|feed| feed.config == cfg) {
                    self.calendar.feeds.retain(|feed| feed.config.provider != cfg.provider);
                    self.calendar.feeds.push(CalendarFeed::new(cfg.clone()));
                }
                self.calendar.config = Some(cfg);
                self.calendar.connected = true;
                self.calendar.failures = 0;
                self.calendar.error.clear();
                self.calendar.last_sync = calendar::now_label();
                self.calendar.test_password.clear();
                self.calendar.form.password.clear();
                self.pending.push(Cmd::CalendarSync);
                if self.page==CALENDAR_PAGE {self.select_calendar_day(self.calendar.view.day);}
            }
            calendar::Event::Synced { config, events, at } => {
                if let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config) {
                    feed.events=events.clone();feed.connected=true;feed.failures=0;feed.error.clear();feed.last_sync=at.clone();
                }
                if self.calendar.config.as_ref()!=Some(&config){return;}
                self.calendar.events = events;
                self.calendar.connected = true;
                self.calendar.failures = 0;
                self.calendar.error.clear();
                self.calendar.last_sync = at;
            }
            calendar::Event::MonthLoaded { config, month, result } => {
                if !self.calendar.feeds.is_empty() {
                    if month!=self.calendar.view.month {return;}
                    let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config) else{return;};
                    feed.month.loading=false;
                    match result {
                        Ok(events)=>{feed.month.events=events;feed.month.loaded=Some(month);feed.month.error.clear();}
                        Err(message)=>feed.month.error=message,
                    }
                    self.refresh_calendar_month_status();
                    return;
                }
                let view=&mut self.calendar.view;
                if month!=view.month || self.calendar.config.as_ref()!=Some(&config){return;}
                view.loading=false;
                match result {
                    Ok(events)=>{view.events=events;view.loaded=Some(month);view.error.clear();}
                    Err(message)=>view.error=message,
                }
            }
            calendar::Event::Failed { config, message } => {
                if let Some(feed)=self.calendar.feeds.iter_mut().find(|feed|feed.config==config) {
                    feed.connected=false;feed.failures=feed.failures.saturating_add(1);feed.error=message.clone();
                }
                if self.calendar.config.as_ref()==Some(&config) {
                    self.calendar.connected = false;
                    self.calendar.failures = self.calendar.failures.saturating_add(1);
                    self.calendar.error = message;
                }
            }
        }
    }

    /// 所有已连接的平台都参与同步，设置标签只决定正在编辑的账号。
    pub fn calendar_configs(&self) -> Vec<calendar::Config> {
        if self.calendar.feeds.is_empty() {self.calendar.config.clone().into_iter().collect()}
        else {self.calendar.feeds.iter().map(|feed|feed.config.clone()).collect()}
    }

    pub(crate) fn refresh_calendar_month_status(&mut self) {
        let month=self.calendar.view.month;
        self.calendar.view.loading=self.calendar.feeds.iter().any(|feed|feed.month.loading);
        self.calendar.view.loaded=(!self.calendar.feeds.is_empty() && self.calendar.feeds.iter().all(|feed|feed.month.loaded==Some(month))).then_some(month);
        self.calendar.view.error=self.calendar.feeds.iter().filter(|feed|!feed.month.error.is_empty())
            .map(|feed|format!("{}：{}",feed.config.provider.label(),feed.month.error)).collect::<Vec<_>>().join("；");
    }

    pub(crate) fn select_calendar_provider(&mut self, provider: calendar::Provider) {
        if self.calendar.testing || self.calendar.form.provider == provider { return; }
        if let Err(error) = calendar::select_provider(provider) {
            self.calendar.note = error;
            self.calendar.note_error = true;
            return;
        }
        let feed=self.calendar.feeds.iter().find(|feed|feed.config.provider==provider);
        self.calendar.config=feed.map(|feed|feed.config.clone());
        self.calendar.events=feed.map(|feed|feed.events.clone()).unwrap_or_default();
        self.calendar.connected=feed.is_some_and(|feed|feed.connected);
        self.calendar.last_sync=feed.map(|feed|feed.last_sync.clone()).unwrap_or_default();
        self.calendar.failures=feed.map(|feed|feed.failures).unwrap_or_default();
        self.calendar.error=feed.map(|feed|feed.error.clone()).unwrap_or_default();
        self.calendar.form=CalendarForm{provider,username:self.calendar.config.as_ref().map(|cfg|cfg.username.clone()).unwrap_or_default(),..Default::default()};
        self.calendar.note.clear();self.calendar.note_error=false;
    }

    /// 只移除正在管理的平台，保留其他平台的日程与提醒。
    pub fn forget_calendar(&mut self) -> Option<calendar::Config> {
        let old = self.calendar.config.take();
        if let Some(config)=&old {
            self.calendar.feeds.retain(|feed|feed.config!=*config);
            let is_old=|alert:&crate::extras::Alert|alert.meeting.as_ref().is_some_and(|(_,source)|source==config.provider.label());
            self.alerts.retain(|alert|!is_old(alert));
            if self.notice.as_ref().is_some_and(is_old){self.notice=None;}
        }
        self.calendar.connected = false;
        self.calendar.events.clear();
        self.calendar.view.events.clear();
        self.refresh_calendar_month_status();
        self.calendar.last_sync.clear();
        self.calendar.failures = 0;
        self.calendar.error.clear();
        self.calendar.note.clear();
        old
    }

    /// 外壳同步收信线程时读账号清单（不含任何凭据）。
    pub fn mail_accounts(&self) -> &[mail::Account] {
        &self.mail.accounts
    }

    /// 账号增删或开关变化后：落盘并让外壳重新对齐收信线程。
    pub(crate) fn mail_accounts_changed(&mut self) {
        mail::save_accounts(&self.mail.accounts);
        self.pending.push(Cmd::MailSync);
    }

    /// 录制/截图用的邮件样例（只在 `record` feature 下编译）。
    ///
    /// `DAOZAI_MAIL_DEMO=1|code` 面板带验证码，`=list` 只列邮件，`=fail` 用假授权码走一次
    /// 「测试连接失败」，方便在没有真实邮箱时验收失败路径。
    #[cfg(feature = "record")]
    pub fn mail_demo(&mut self) {
        use mail::{Account, Item, Provider, Status};
        let mode = std::env::var("DAOZAI_MAIL_DEMO").unwrap_or_default();
        self.settings_tab = 7;
        if mode == "fail" {
            self.mail.form.provider = Provider::Qq;
            self.mail.form.email = "test@qq.com".into();
            self.mail.form.password = "invalid-auth-code".into();
            self.mail.note = "test@qq.com：正在测试连接…".into();
            self.pending.push(Cmd::MailTest(
                Account { email: "test@qq.com".into(), provider: Provider::Qq, enabled: true, server: None, port: None },
                "invalid-auth-code".into(),
            ));
            return;
        }
        self.mail.accounts = vec![
            Account { email: "tang@qq.com".into(), provider: Provider::Qq, enabled: false, server: None, port: None },
            Account { email: "hi@icloud.com".into(), provider: Provider::ICloud, enabled: false, server: None, port: None },
        ];
        for a in &self.mail.accounts {
            self.mail.status.insert(a.email.clone(), Status::Ready);
        }
        // 会议演示：开始时间设成 10 分钟后，正好落在默认 15 分钟提前量里，
        // 启动即触发一次「会议提醒」。
        let mstart = chrono::Local::now().timestamp() + 600;
        let mat = chrono::DateTime::from_timestamp(mstart, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let rows = [
            (
                "GitHub",
                "[GitHub] Please verify your device",
                "09:41",
                Some("482913"),
                "We noticed a sign-in attempt from a new device. Enter 482913 to finish signing in. If this wasn't you, secure your account now.",
                None,
            ),
            (
                "Apple",
                "Your Apple ID verification code",
                "09:38",
                None,
                "Your Apple ID code is 573104. Don't share it with anyone. If you didn't request it, you can ignore this message.",
                None,
            ),
            (
                "腾讯企业邮",
                "会议邀请：周五方案评审",
                "09:12",
                None,
                "本周五 14:30 在三楼会议室评审 v0.5.5 方案，请提前十分钟到场。会议邀请见附件。",
                Some((mstart, mat)),
            ),
            (
                "顺丰速运",
                "您的快件已到达菜鸟驿站",
                "08:57",
                None,
                "您的快件已到达菜鸟驿站（望京 SOHO T1 店），请凭取件码 6-3-2 在 20:00 前领取。",
                None,
            ),
        ];
        let with_code = mode != "list";
        self.mail.recent = rows
            .iter()
            .enumerate()
            .map(|(i, (from, subject, time, code, summary, meeting))| Item {
                account: "tang@qq.com".into(),
                from: (*from).into(),
                subject: (*subject).into(),
                time: (*time).into(),
                code: if with_code { (*code).map(str::to_string) } else { None },
                summary: (*summary).into(),
                links: Vec::new(),
                meeting: meeting.as_ref().map(|(start, at)| mail::Meeting { all_day:false, end:None, title: "周五方案评审".into(), start: *start, at: at.clone(), uid: "demo-ics-1".into() }),
                uid: 900 - i as u32,
            })
            .collect();
        if let Some(code) = self.mail.recent.iter().find_map(|m| m.code.clone()) {
            self.code_seen = Some((code, self.t));
        }
        self.mail.note = "tang@qq.com：连接成功".into();
        self.page = 5;
        self.dots_target();
    }

    /// 录制/截图用的日程样例（只在 `record` feature 下编译）。
    ///
    /// `DAOZAI_CAL_DEMO=paste|fail|connected|alert`。
    #[cfg(feature = "record")]
    pub fn calendar_demo(&mut self) {
        use calendar::Provider;
        let mode = std::env::var("DAOZAI_CAL_DEMO").unwrap_or_default();
        self.calendar.form.provider = Provider::Feishu;
        self.calendar.form.username = "demo***@feishu.example".into();
        self.settings_tab = 8;
        self.open_settings();
        self.settings.calendar_prompt = false;
        let later = |secs: i64| chrono::Local::now().timestamp() + secs;
        let label = |ts: i64| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
                .unwrap_or_default()
        };
        let demo_cfg = calendar::Config {
            provider: Provider::Feishu,
            username: "demo***@feishu.example".into(),
            href: Some("https://caldav.feishu.cn/calendars/8f3c2d7a-1111-2222-3333-444455556666/".into()),
            server: None,
        };
        match mode.as_str() {
            "paste" => {
                self.calendar.form.username =
                    "服务器地址：https://caldav.feishu.cn/calendars/\n用户名：demo***@feishu.example\n密码：demo-pass-123".into();
                if let Some((u, p)) = calendar::parse_paste(&self.calendar.form.username) {
                    self.calendar.form.username = u;
                    self.calendar.form.password = p;
                    self.calendar.note = "已从粘贴内容拆出用户名和密码，服务器地址已按平台填好".into();
                }
            }
            "fail" => {
                self.calendar.form.password = "demo-pass-123".into();
                self.calendar.note = calendar::human_error(Provider::Feishu, Some(401));
                self.calendar.note_error = true;
            }
            "connected" => {
                let a = later(3600);
                let b = later(6 * 3600);
                self.calendar.form.password = "demo-pass-123".into();
                self.calendar.events = vec![
                    mail::Meeting { all_day:false, end:None, title: "产品评审".into(), start: a, at: label(a), uid: "demo-cal-1".into() },
                    mail::Meeting { all_day:false, end:None, title: "设计周会".into(), start: b, at: label(b), uid: "demo-cal-2".into() },
                ];
                self.calendar.config = Some(demo_cfg.clone());
                self.calendar.connected = true;
                self.calendar.last_sync = calendar::now_label();
                self.calendar.note.clear();
                self.calendar.note_error = false;
            }
            "alert" => {
                let s = later(600);
                self.calendar.events =
                    vec![mail::Meeting { all_day:false, end:None, title: "飞书日程：产品评审".into(), start: s, at: label(s), uid: "demo-cal-alert".into() }];
                self.calendar.config = Some(demo_cfg.clone());
                self.calendar.connected = true;
                self.calendar.last_sync = calendar::now_label();
                self.settings_open = false;
            }
            _ => {}
        }
    }

    /// v0.5.4：把系统音量与蓝牙设备状态同步进界面；只在连接/断开时提示一次。
    fn sync_device(&mut self, env: &Env) {
        if self.t >= self.volume_quiet_until {
            if let Some(v) = env.volume {
                self.volume_ui = Some(v);
            }
        }
        if !env.bt_ready { return; }
        // 只把首次完整扫描作为基线，之后的连接变化都应提示。
        if !self.bt_initialized {
            self.bt_initialized = true;
            self.bt_shown = env.bt.clone();
            return;
        }
        let connected: Vec<_> = env.bt.iter().filter(|d| !self.bt_shown.iter().any(|old| old.id == d.id)).cloned().collect();
        let disconnected: Vec<_> = self.bt_shown.iter().filter(|d| !env.bt.iter().any(|new| new.id == d.id)).cloned().collect();
        for device in &connected { self.bluetooth_alert(device, true); }
        for device in &disconnected { self.bluetooth_alert(device, false); }
        self.bt_shown = env.bt.clone();
    }

    /// 距离上次交互的秒数（小精灵出现的依据）。
    pub fn idle_for(&self) -> f64 {
        self.system_idle.unwrap_or(self.t - self.last_input)
    }

    pub fn show_page(&mut self,page:usize) {
        if !self.settings.visible_pages[page] {
            self.settings_tab=[1,2,3,5,4,7,11,8][page];
            self.open_settings();
            return;
        }
        self.notice=None;
        self.go(page,1.0);
        self.open=true; self.forced_open_until=self.t+4.0;
        self.w.tune(OPEN_W.0,OPEN_W.1);self.h.tune(OPEN_H.0,OPEN_H.1);
        self.w.target=PW;self.h.target=self.expanded_height();
    }
    /// 点击胶囊直接展开（不换页、不消通知），并保底展开 4 秒。
    pub(crate) fn open_now(&mut self) {
        self.open = true;
        // 不用 forced_open_until 兜底：鼠标在岛上靠 stay_open 保持，离开后由设置里的「移开多久收起」决定。
        self.hover_since = None;
        self.leave_since = None;
        self.w.tune(OPEN_W.0, OPEN_W.1);
        self.h.tune(OPEN_H.0, OPEN_H.1);
        self.w.target = PW;
        self.h.target = self.expanded_height();
    }
    pub fn open_settings(&mut self){
        self.settings_open=true;
        self.settings_focus_requested=true;
    }
    pub fn open_settings_category(&mut self,category:usize){
        self.settings_category=category.min(3);
        if category==0 || category>=3 {self.settings_tab=if category==0{0}else{6};self.open_settings();return;}
        let page=if category==1{2}else{1};
        self.show_page(page);
        if self.settings.visible_pages[page] && (category==1 || category==2) {self.subpage=Some(page);}
    }
    pub fn page(&self) -> usize {
        self.page
    }

    pub(crate) fn set_page_visible(&mut self,page:usize,visible:bool) {
        if !visible && self.settings.visible_pages.iter().filter(|enabled|**enabled).count()==1
            && self.settings.visible_pages[page] { return; }
        self.settings.visible_pages[page]=visible;
        if !self.settings.visible_pages[self.page] {
            if let Some(first)=self.settings.page_order.iter().copied().find(|page|self.settings.visible_pages[*page]) { self.go(first,1.0); }
        }
    }

    fn navigate_page(&mut self,next:bool) {
        let position=self.settings.page_order.iter().position(|page|*page==self.page).unwrap();
        for offset in 1..=PAGE_NAMES.len() {
            let page=self.settings.page_order[(position+if next {offset}else{PAGE_NAMES.len()-offset})%PAGE_NAMES.len()];
            if self.settings.visible_pages[page] {
                self.go(page,if next {1.0}else{-1.0});
                break;
            }
        }
    }

    pub fn is_editing(&self) -> bool {
        self.adding || self.editing.is_some() || self.task_dialog.is_some() || self.calendar.draft.is_some()
    }

    pub fn foreground_changed(&mut self) {
        self.hover_lock=true;
        if self.adding && self.buf.trim().is_empty(){self.cancel_edit();}
    }

    /// 带过渡地切到某页。`dir` > 0 表示内容从右侧进入。
    pub(crate) fn go(&mut self, page: usize, dir: f32) {
        if page == self.page || !self.settings.visible_pages[page] { return; }
        self.subpage=None;
        // 连点：上一段过渡还没结束时，以当前页为起点重新开始，位移速度保留。
        self.page_from = self.page;
        self.page = page;
        if page==CALENDAR_PAGE {self.request_calendar_month(false);}
        self.page_dir = dir;
        self.page_t0 = self.t;
        self.slide.pos = dir * PAGE_SLIDE + if self.page_e() < 0.4 { self.slide.pos * 0.3 } else { 0.0 };
        self.slide.target = 0.0;
        self.dots_target();
        self.cancel_edit();
    }

    fn page_e(&self) -> f32 {
        (self.t - self.page_t0) as f32
    }

    fn dots_target(&mut self) {
        for (i, d) in self.dots.iter_mut().enumerate() {
            d.target = if i == self.page { 16.0 } else { 4.0 };
        }
    }

    pub(crate) fn cancel_edit(&mut self) {
        self.adding = false;
        self.editing = None;
        self.task_dialog=None;
        self.calendar.draft=None;
        self.buf.clear();
        self.todo_input_focused=false;
    }

    fn island_lyrics_active(&self)->bool {
        self.settings.island_lyrics && self.settings.visible_pages[0] && self.media.is_music()
            && !self.browser_minimal_active
            && self.ly.key==format!("{}|{}|{:.0}",self.media.title,self.media.artist,self.media.duration)
            && self.ly.lyrics.as_ref().is_some_and(|lyrics|lyrics.synced && lyrics.lines.iter().any(|line|!line.text.trim().is_empty()))
    }
    pub(crate) fn compact_notices(&self)->bool {self.island_lyrics_active()&&!self.open&&!self.notice.as_ref().is_some_and(|a|a.meeting.is_some())}

    /// 换句、分段和展开高亮共用时间调整；正数延后，负数提前。
    pub(crate) fn lyric_position(&self)->f32 {(self.media.position-self.settings.lyric_delay_ms as f64/1000.0)as f32}

    pub(crate) fn island_lyric(&self) -> Option<(usize,&str)> {
        if !self.island_lyrics_active(){return None;}
        if self.ly.key!=format!("{}|{}|{:.0}",self.media.title,self.media.artist,self.media.duration) {return None;}
        let lyrics=self.ly.lyrics.as_ref().filter(|lyrics|lyrics.synced)?;
        let index=lyrics.index_at(self.lyric_position())?;
        let text=lyrics.lines.get(index)?.text.trim();
        (!text.is_empty()).then_some((index,text))
    }

    fn update_island_lyric_layout(&mut self,ctx:&egui::Context) {
        if !self.island_lyrics_active(){self.island_lyric_layout=None;return;}
        let current=self.island_lyric();
        let text=if let Some((_,line))=current {line}
            else if self.ly.loading || self.ly.key!=format!("{}|{}|{:.0}",self.media.title,self.media.artist,self.media.duration) {"正在查找歌词…"}
            else if let Some(lyrics)=&self.ly.lyrics {
                if !lyrics.synced {"暂无同步歌词"}
                else if lyrics.index_at(self.lyric_position()).is_none() {"前奏"}
                else {"间奏"}
            } else {""};
        let ink=if current.is_some(){theme::WHITE}else{theme::GRAY};
        let layout=|text:String| {
            let mut job=egui::text::LayoutJob::simple(text,theme::font(11.0,W::Semibold),ink,LYRIC_BAR_W-80.0);
            job.halign=egui::Align::Center;
            job.wrap.break_anywhere=true;
            ctx.fonts(|fonts|fonts.layout_job(job))
        };
        let mut galley=layout(text.to_owned());
        if let Some((index,_))=current.filter(|_|galley.rows.len()>1) {
            let lyrics=self.ly.lyrics.as_ref().unwrap();
            let start=lyrics.lines[index].t;
            let end=lyrics.lines.iter().skip(index+1).find(|line|line.t>start).map_or(self.media.duration as f32,|line|line.t);
            let progress=if end>start {((self.lyric_position()-start)/(end-start)).clamp(0.0,1.0)}else{0.0};
            // 按当前句的时间范围与各段字数切换，暂停和跳转直接跟随播放位置。
            let total:usize=galley.rows.iter().map(|row|row.glyphs.len()).sum();
            let mut count=0;
            let page=galley.rows.iter().position(|row|{count+=row.glyphs.len();progress*(total as f32)<count as f32}).unwrap_or(galley.rows.len()-1);
            let segment=galley.rows[page].glyphs.iter().map(|glyph|glyph.chr).collect::<String>();
            galley=layout(segment.trim().to_owned());
        }
        self.island_lyric_layout=Some(galley);
    }

    /// 是否处于「空闲」：没有展开、提醒、铃铛、吐司、编辑或岛上歌词。
    /// 刻意不看后台活动（音乐/专注/验证码），否则小药丸/隐入在多数机器上永不生效。
    fn idle(&self)->bool {
        !self.open
            && self.notice.is_none()
            && self.toast.is_none()
            && self.t >= self.notification_bell_until
            && !self.island_lyrics_active()
            && !self.is_editing()
    }
    /// 用户选择的目标形态（不看提示状态）。
    fn selected_raw(&self)->Present {
        match self.settings.idle_mode {
            model::IdleMode::Bar => Present::Bar,
            model::IdleMode::Drop => Present::Pill,
            model::IdleMode::Hidden => Present::Hidden,
        }
    }
    /// 「完全隐入」在光标贴顶（且靠近岛体中心）时临时冒成小药丸。
    fn selected_present(&self)->Present {
        match self.selected_raw() {
            Present::Hidden if self.near_top_now => Present::Pill,
            other => other,
        }
    }
    fn previewing(&self)->bool { self.t < self.idle_preview_until }
    /// 是否应收成小药丸。
    fn idle_pill(&self)->bool {
        if self.previewing() { return matches!(self.selected_raw(), Present::Pill); }
        self.idle() && matches!(self.selected_present(), Present::Pill)
    }
    /// 是否应完全隐入（窗口裁剪为空）。
    fn idle_hidden(&self)->bool {
        if self.previewing() { return matches!(self.selected_raw(), Present::Hidden); }
        self.idle() && matches!(self.selected_present(), Present::Hidden)
    }
    /// 是否强制细线：浏览器避让；或手动「极简刘海」且空闲形态为「黑条」。
    /// 手动极简不再覆盖小药丸/隐入，否则用户选了没效果。
    fn thin_forced(&self)->bool {
        // 只有用户选的是「黑条」时，极简/避让才会把它压成细线。
        // 选了「小药丸 / 完全隐入」就以用户选择为准，否则会被避让一直压成细线，
        // 看起来就像"小药丸没生效"。
        !self.previewing()
            && self.settings.idle_mode == model::IdleMode::Bar
            && (self.browser_minimal_active || self.settings.minimal)
    }
    /// 切换空闲形态后强制预览（在设置里点选时调用）。
    pub(crate) fn preview_idle_mode(&mut self) {
        self.idle_preview_until = self.t + 5.0;
        // 手动「极简刘海」会覆盖「黑条」，切换时顺手关掉，避免用户以为设置没生效。
        self.settings.minimal = false;
    }
    fn effective_minimal(&self)->bool { self.thin_forced() || self.idle_pill() }
    pub(crate) fn pet_enabled(&self)->bool {self.settings.pet && !self.effective_minimal()}
    pub(crate) fn pet_space(&self)->f32 {if self.pet_enabled(){44.0}else{0.0}}
    fn lyric_focus_space(&self)->f32 {if self.pomo.running{crate::focus_bar::LYRIC_WIDTH+10.0}else{0.0}}
    fn lyric_bar_width(&self)->f32 {LYRIC_BAR_W+if self.pet_enabled(){48.0}else{0.0}+self.lyric_focus_space()}
    pub(crate) fn notice_size(&self,a:&Alert)->(f32,f32) {
        let (width,height)=if a.meeting.is_some(){(360.0,136.0)}else if a.title=="蓝牙设备"{(HUD_W,48.0)}else if a.is_hud(){(HUD_W,38.0)}else{(334.0,44.0)};
        (width,height)
    }
    pub fn set_pet(&mut self,enabled:bool) {
        self.settings.pet=enabled;if enabled {self.settings.minimal=false;}self.settings.save();
    }
    pub fn set_minimal(&mut self,enabled:bool) {
        self.settings.minimal=enabled;if enabled {self.settings.pet=false;}self.settings.save();
    }
    fn base_size(&self) -> (f32, f32) {
        if !self.compact_notices()&&!self.open {
            if let Some(a)=&self.notice {let (width,height)=self.notice_size(a);return (width+self.pet_space(),height);}
        }
        if self.island_lyrics_active() {
            (self.lyric_bar_width(),BAR_H)
        } else if self.effective_minimal() && (self.previewing() || self.t >= self.notification_bell_until) {
            if self.thin_forced() {
                (BAR_W_MIN, BAR_H_MIN)
            } else if self.idle_pill() {
                // 水滴：先收成小药丸，再随 idle_drop 收成一颗小水珠融入顶部。
                let e = smoothstep(0.0, 1.0, self.idle_drop);
                let w = IDLE_PILL_W + (IDLE_DROP_W - IDLE_PILL_W) * e;
                let h = IDLE_PILL_H + (IDLE_DROP_H - IDLE_PILL_H) * e;
                (w, h)
            } else {
                (IDLE_PILL_W, IDLE_PILL_H)
            }
        } else {
            (self.resident.target.width, BAR_H)
        }
    }

    /// 0 = 药丸，1 = 完整面板（展开时可略超过 1）。
    #[cfg(test)]
    pub(crate) fn current_width(&self) -> f32 { self.w.pos+self.notice_width.pos }

    pub fn progress(&self) -> f32 {
        let (_, bh) = self.base_size();
        (self.h.pos - bh) / (self.expanded_height() - bh)
    }

    pub(crate) fn expanded_height(&self)->f32 {
        if self.page==CALENDAR_PAGE {CALENDAR_H}else if self.page==5&&self.mail.open_uid.is_some(){236.0}else{PH}
    }

    // ---------------------------------------------------------------
    // 灵动岛：悬停 peek / 长按快捷操作 / 岛史 / 夜间 / 会议勿扰
    // ---------------------------------------------------------------
    /// 会议 / 投屏勿扰：仅 Windows 下检测常见会议软件进程。
    /// 进程枚举有开销，按 ~1.5s 缓存一次结果，避免每帧扫全进程表。
    fn dnd_active(&mut self, t: f64) -> bool {
        if !self.settings.dnd_meeting {
            self.dnd_cache = false;
            return false;
        }
        if t - self.dnd_checked_at > 1.5 {
            self.dnd_checked_at = t;
            #[cfg(windows)]
            { self.dnd_cache = crate::system::meeting_running(); }
            #[cfg(not(windows))]
            { self.dnd_cache = false; }
        }
        self.dnd_cache
    }

    /// 由本地时间平滑得到的夜间系数（0 白天，1 深夜）。22:30 后渐入，07:30 前渐出。
    fn night_target(&self) -> f32 {
        if !self.settings.night_mode { return 0.0; }
        let h = self.current_hour;
        let m = self.current_minute as f32 / 60.0;
        let t = h as f32 + m; // 0..24
        let up = 7.5;   // 07:30 亮
        let down = 22.5; // 22:30 暗
        if t >= up && t <= down {
            0.0
        } else if t > down {
            smoothstep(down, down + 1.0, t)
        } else {
            // 0..up：从 1 渐退到 0（0:00 最暗）
            smoothstep(up - 1.0, up, t)
        }
    }

    /// 活动的稳定签名，用于「新活动出现才响一声」。
    fn activity_key(a: &Activity) -> String {
        match a.kind {
            Kind::Focus => format!("focus:{}", a.title),
            Kind::Media => format!("media:{}", a.title),
            _ => format!("{:?}:{}", a.kind, a.detail),
        }
    }

    /// 屏上最多同时表达 2 个活动（主 + 分裂胶囊）；其余（第 3 个起）滚入岛史。
    fn update_island_history(&mut self, activities: &[Activity]) {
        for a in activities.iter().skip(2) {
            let key = Self::activity_key(a);
            if self.island_history.iter().any(|h| Self::activity_key(h) == key) {
                continue;
            }
            self.island_history.push_back(a.clone());
            // 最多保留 24 条，超出丢弃最早的。
            while self.island_history.len() > 24 {
                self.island_history.pop_front();
            }
        }
    }

    /// 长按后弹出的快捷操作条；点击把副作用交给外壳（Cmd）或界面自身。
    fn draw_quick_menu(&mut self, ui: &mut egui::Ui, cx: f32, top: f32, cmds: &mut Vec<Cmd>) {
        let Some(kind) = self.quick_menu else { return };
        let gap = 6.0;
        let y = top + BAR_H + 8.0;
        let painter = ui.painter_at(ui.max_rect().expand(40.0));
        // 按钮标签；动作在点击时按标签分发。
        let labels: Vec<&str> = match kind {
            Kind::Media => vec!["上一首", "播放/暂停", "下一首"],
            Kind::Code => {
                let mut v = Vec::new();
                if self.code_seen.is_some() { v.push("复制"); }
                v.push("看邮件");
                v
            }
            Kind::Focus => vec!["暂停/继续", "重置"],
            _ => vec!["打开"],
        };
        let bw = 86.0;
        let bh = 28.0;
        let total_w = labels.len() as f32 * bw + (labels.len().max(1) as f32 - 1.0) * gap;
        let x0 = cx - total_w / 2.0;
        let rect = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(total_w, bh));
        shape::shadow(&painter, rect, 0.0, 0.0, 10.0, 0.5);
        painter.rect_filled(rect, 10.0, Color32::from_black_alpha(220));
        for (i, label) in labels.into_iter().enumerate() {
            let bx = x0 + i as f32 * (bw + gap);
            let b = Card::new(ui, Pos2::new(bx, y), ui.max_rect());
            let hit = b.hit(0.0, 0.0, bw, bh, format!("quick-{i}"));
            let col = if hit.hovered() { theme::alpha_accent(0.9) } else { theme::accent() };
            b.centered_text(bw / 2.0, bh / 2.0 + 4.0, label, 12.0, W::Semibold, col, H::Center);
            if hit.clicked() {
                match label {
                    "上一首" => cmds.push(Cmd::Media(Transport::Prev)),
                    "下一首" => cmds.push(Cmd::Media(Transport::Next)),
                    "播放/暂停" => cmds.push(Cmd::Media(Transport::Toggle)),
                    "复制" => { if let Some((code, _)) = &self.code_seen { cmds.push(Cmd::Copy(code.clone())); } }
                    "看邮件" => self.show_page(5),
                    "打开" => self.show_page(self.side_slot.as_ref().map(|a| a.page).unwrap_or(0)),
                    "暂停/继续" => self.toggle_pomo(),
                    "重置" => self.reset_pomo(),
                    _ => {}
                }
                self.quick_menu = None;
            }
        }
    }

    fn toggle_pomo(&mut self) {
        self.pomo.running = !self.pomo.running;
    }
    fn reset_pomo(&mut self) {
        self.pomo.running = false;
        self.pomo.remaining = self.settings.focus_minutes as f32 * 60.0;
    }

    // ---------------------------------------------------------------
    // 每帧入口
    // ---------------------------------------------------------------
    pub fn frame(&mut self, ui: &mut egui::Ui, win: Rect, env: &Env) -> FrameOut {
        let ctx = ui.ctx().clone();
        // 外观设置可能从设置页、配置文件或快捷键改，每帧对齐一次就好。
        if (ctx.zoom_factor() - self.settings.ui_zoom).abs() > 0.0005 {
            ctx.set_zoom_factor(self.settings.ui_zoom);
        }
        if self.settings.accent != self.accent_applied {
            apply_accent(&self.settings.accent);
            self.accent_applied = self.settings.accent.clone();
            ctx.request_repaint();
        }
        let dt = match self.last_t {
            Some(t0) => (env.t - t0).clamp(0.0, 0.1) as f32,
            None => 0.0,
        };
        let elapsed = self.last_t.map(|at| (env.t - at).max(0.0) as f32).unwrap_or(0.0);
        self.last_t = Some(env.t);
        self.t = env.t;
        self.quota_tick();
        self.current_hour=env.hour();
        self.current_minute=env.minute();
        self.system_idle=env.idle_seconds;
        self.dt = dt;
        if let Some(at_top)=env.browser_at_top {self.browser_at_top=at_top;}
        let browser_minimal=self.settings.browser_minimal && self.browser_at_top;
        if browser_minimal && !self.browser_minimal_active {
            self.open=false;
            self.hover_lock=true;
            self.hover_since=None;
            self.leave_since=None;
            self.forced_open_until=0.0;
        }
        self.browser_minimal_active=browser_minimal;
        if !self.started {
            self.started = true;
            self.last_input = env.t;
            self.last_reminder = env.t;
            self.page_t0 = env.t - 10.0;
            if self.page==CALENDAR_PAGE {self.request_calendar_month(false);}
        }
        let mut cmds = Vec::new();

        self.tick(env, dt, elapsed);
        self.sync_device(env);
        self.tick_alerts();
        let hud_active=self.notice.clone();
        if hud_active.as_ref().map(|a|&a.title)!=self.hud_notice.as_ref().map(|a|&a.title)||hud_active.is_none(){self.notice_details=false;}
        if let Some(a)=&hud_active {self.hud_notice=Some(a.clone());}
        self.hud_alpha=anim::approach(self.hud_alpha,if hud_active.is_some(){1.0}else{0.0},if hud_active.is_some(){16.0}else{8.0},dt);
        if hud_active.is_none() && self.hud_alpha<0.003 {self.hud_notice=None;}
        // 统一活动卡：主岛身表达优先级最高的一条，下一条进入右侧分裂小胶囊。
        let activities=self.collect_activities();
        // 验证码遮罩：记录首次见到时间，并清理已消失的条目。
        if let Some(item)=self.mail.code_item(){ self.privacy.observe(item,self.t); }
        for item in &self.mail.recent { self.privacy.observe(item,self.t); }
        self.privacy.retain(&self.mail.recent);
        let side=(!self.open).then(||self.side_activity(&activities)).flatten();
        self.side_slot=side.clone();
        let badge_active=self.compact_notices()&&(self.notice.is_some()||self.t<self.notification_bell_until);
        let notice_target=match &side {
            Some(a)=>a.pill_width(),
            None=>if badge_active {44.0}else{0.0},
        };
        // 出现用欠阻尼弹簧（挤出的回弹），消失用临界阻尼（安静地合回岛体）。
        if notice_target>0.0 && self.notice_width.target<=0.0 {
            self.notice_width.tune(340.0,0.68);
            self.side_gap.tune(300.0,0.62);
        } else if notice_target<=0.0 {
            self.notice_width.tune(CLOSE.0,CLOSE.1);
            self.side_gap.tune(CLOSE.0,CLOSE.1);
        }
        self.notice_width.target=notice_target;
        self.side_gap.target=if notice_target>0.0 {8.0}else{0.0};
        self.side_gap.step(dt);
        if notice_target>0.0 {self.notice_width.step(dt);}else{self.notice_width.snap(0.0);self.notice_details=false;}
        self.update_media(env,&ctx);
        let bell=self.t<self.notification_bell_until&&!self.compact_notices();
        self.resident_power=(env.battery,env.charging);
        let mut layout=Layout::new(self.settings.show_battery&&env.battery.is_some(),self.quota_bar_indices().len(),self.media.is_music()&&!bell,self.pomo.running,bell,self.pet_enabled());
        layout.reorder(&self.settings.resident_order);
        if self.pet_enabled() {
            if let Some(a)=self.notice.as_ref().filter(|_|!self.compact_notices()&&!self.open){layout.x[bar::PET]=Some(22.0-(self.notice_size(a).0+self.pet_space())/2.0);}
            else if self.island_lyrics_active() {layout.x[bar::PET]=Some(60.0-self.lyric_bar_width()/2.0);}
        }
        self.resident.step(layout,dt,ctx.style().animation_time>0.0&&bar::animations_enabled());
        if self.opts.fetch_lyrics {
            self.poll_lyrics();
        }
        self.update_island_lyric_layout(&ctx);

        let cx = win.center().x;
        let top = win.top();

        // ---------- 输入：拖放 ----------
        let (hovering_files, dropped) = ctx.input(|i| {
            (
                self.settings.visible_pages[3] && !i.raw.hovered_files.is_empty(),
                i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>(),
            )
        });
        if !dropped.is_empty() && self.settings.visible_pages[3] {
            self.on_drop(dropped);
            self.show_page(3);
        } else if hovering_files && !self.drag_hot {
            self.drag_hot = true;
            if self.page != 3 {
                self.drag_saved = Some(self.page);
                self.go(3, if self.page < 3 { 1.0 } else { -1.0 });
            }
        } else if !hovering_files && self.drag_hot {
            // 拖到外面松手 / 取消：安静地退回原页
            self.drag_hot = false;
            if let Some(p) = self.drag_saved.take() {
                self.go(p, if p < 3 { -1.0 } else { 1.0 });
            }
        }
        self.drag_a = anim::approach(self.drag_a, if self.drag_hot { 1.0 } else { 0.0 }, 16.0, dt);

        // ---------- 输入：悬停 ----------
        let p_prev = self.progress();
        let hit = self.hit_rect(cx, top, p_prev);
        let inside = env.cursor.is_some_and(|c| hit.contains(c));
        let on_notice=env.cursor.is_some_and(|c|self.notice_rect(cx,top).is_some_and(|r|r.contains(c))||self.notice_details_rect(cx,top).is_some_and(|r|r.contains(c)));
        if !inside {
            self.hover_lock = false;
        }
        if env.cursor != self.last_cursor || ctx.input(|i| !i.events.is_empty()) {
            if inside || hovering_files || ctx.input(|i| !i.events.is_empty()) {
                self.last_input = env.t;
            }
            self.last_cursor = env.cursor;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.is_editing() {
            self.cancel_edit();
        }
        if !self.is_editing() && inside && self.open && self.reveal > 0.95 {
            let (scroll, left, right, esc) = ctx.input(|i| (i.raw_scroll_delta.y, i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Escape)));
            if esc { self.hover_lock = true; }
            if left || right {
                self.wheel_delta=0.0;
                self.navigate_page(right);
            } else if scroll!=0.0 {
                let cursor=env.cursor.unwrap()-Vec2::new(cx-PW/2.0,top);
                let mail_body=self.page==5 && self.mail.open_uid.is_some()
                    && Rect::from_min_max(Pos2::new(46.0,104.0),Pos2::new(374.0,194.0)).contains(cursor);
                let mail_list=self.page==5 && self.mail.open_uid.is_none() && self.mail.recent.len()>3 && Rect::from_min_max(Pos2::new(182.0,36.0),Pos2::new(380.0,122.0)).contains(cursor);
                if mail_body || mail_list {self.wheel_delta=0.0;}
                else {
                    // 累积高精度滚轮的小幅输入；停顿或反向时丢弃上一段余量。
                    if self.t-self.wheel_at>0.18 || self.wheel_delta*scroll<0.0 {self.wheel_delta=0.0;}
                    self.wheel_at=self.t;
                    self.wheel_delta+=scroll;
                    if self.wheel_delta.abs()>=8.0 {
                        let next=self.wheel_delta<0.0;
                        self.wheel_delta=0.0;
                        let calendar_len=if self.page==CALENDAR_PAGE {self.calendar_entries(self.calendar.view.day).len()}else{0};
                        let calendar_body=self.page==CALENDAR_PAGE && Rect::from_min_max(Pos2::new(163.0,52.0),Pos2::new(378.0,208.0)).contains(cursor);
                        let list=match self.page {
                            CALENDAR_PAGE if calendar_body=>Some((&mut self.calendar.view.first,calendar_len.saturating_sub(4))),
                            4 if Rect::from_min_max(Pos2::new(196.0,31.0),Pos2::new(377.0,124.0)).contains(cursor)
                                =>Some((&mut self.todo_first,self.todos.items.len().saturating_sub(3))),
                            3 if Rect::from_min_max(Pos2::new(36.0,28.0),Pos2::new(383.0,128.0)).contains(cursor)
                                =>Some((&mut self.shelf_first,self.shelf.items.len().saturating_sub(5))),
                            _=>None,
                        };
                        let scrolled=list.is_some_and(|(first,max)|{
                            let target=if next {(*first+1).min(max)}else{first.saturating_sub(1)};
                            let changed=target!=*first;*first=target;changed
                        });
                        // 内容区优先滚动列表，空列表和边界继续翻页；连续滚动保留短暂间隔。
                        if !scrolled && !calendar_body && self.t-self.wheel_page_at>=0.20 {
                            self.navigate_page(next);
                            self.wheel_page_at=self.t;
                        }
                        ctx.input_mut(|i|{i.raw_scroll_delta=Vec2::ZERO;i.smooth_scroll_delta=Vec2::ZERO;});
                    }
                }
            }
        } else {self.wheel_delta=0.0;}
        // --- 灵动岛：会议勿扰 / 顶部感应 / peek / 长按 / 岛史 / 夜间 / 音效 ---
        self.dnd_now = self.dnd_active(env.t);
        // 光标要同时「贴顶」且「横向在岛体中心附近」才算靠近，才冒头/预览。
        // 只判 Y 的话，鼠标停在任意窗口的标题栏、浏览器标签栏都会误触发，
        // 「完全隐入」就会一直冒出一颗小药丸，看起来像没生效。
        let near_top = !self.open && match env.cursor {
            Some(c) => c.y <= 48.0 && (c.x - cx).abs() <= 220.0,
            None => crate::input::cursor_near_top(64),
        };
        self.near_top_now = near_top;
        let hovering = inside || near_top;
        // peek：悬停 300ms 后轻微放大预览（仅在收起态生效）。
        if !self.open && self.settings.peek_enabled && hovering {
            let since = *self.peek_since.get_or_insert(env.t);
            if env.t - since >= 0.3 { self.peek.target = 1.0; }
        } else {
            self.peek_since = None;
            self.peek.target = 0.0;
        }
        self.peek.step(dt);
        // 长按胶囊弹出快捷操作。
        // 注意：any_pressed() 只在按下那一帧为真，所以长按计时必须每帧检查「按住」状态，
        // 而不是只在按下帧判定，否则 0.5s 阈值永远凑不满、菜单永不弹出。
        let pointer_down = ctx.input(|i| i.pointer.any_pressed());
        let pointer_up = ctx.input(|i| i.pointer.any_released());
        let pointer_held = ctx.input(|i| i.pointer.any_down());
        let over_capsule = inside || self.notice_rect(cx, top).is_some_and(|r| env.cursor.is_some_and(|c| r.contains(c)));
        if pointer_down && !self.open && over_capsule {
            self.press_since = Some(env.t);
        }
        if pointer_held && !self.open && over_capsule {
            if let Some(since) = self.press_since {
                if env.t - since >= 0.5 {
                    if let Some(k) = activities.first().map(|a| a.kind) { self.quick_menu = Some(k); }
                }
            }
        }
        if pointer_up {
            // 短按（<0.5s）且点在胶囊上：像手机点灵动岛一样，直接展开完整面板。
            // 之前只有「悬停展开」，用户点击黑线/黑条毫无反应，与真机不符。
            let mut tap_open = false;
            if let Some(since) = self.press_since {
                if env.t - since < 0.5 {
                    self.quick_menu = None;
                    tap_open = true;
                }
            }
            self.press_since = None;
            // 只在「空闲的收起态胶囊」上点开；通知横幅/侧边胶囊上有自己的按钮，
            // 若这里也强制展开会改变布局、把用户点的按钮挪走。
            if tap_open && !self.open && inside && self.notice.is_none() && !self.is_editing() { self.open_now(); }
        }
        if activities.is_empty() { self.quick_menu = None; self.history_open = false; }
        // 岛史：屏上放不下的次级活动滚入历史。
        self.update_island_history(&activities);
        // 夜间系数平滑过渡。
        self.night = anim::approach(self.night, self.night_target(), 4.0, dt);
        // 新活动出现：轻响一声（会议勿扰时不响）。
        let sig: Vec<String> = activities.iter().map(Self::activity_key).collect();
        if !sig.is_empty() && sig.iter().any(|k| !self.sound_sig.contains(k)) && self.settings.sound && !self.dnd_now && env.t - self.sound_at > 0.25 {
            sfx::play(sfx::Sfx::Pop);
            self.sound_at = env.t;
        }
        self.sound_sig = sig;

        // 平时隐藏：鼠标扫过不展开，只认通知到期时间（forced_open_until）和主动点击。
        // 会议 / 投屏勿扰时也不自动弹出，只留小圆。
        // 「完全隐入」模式鼠标贴顶时冒头，属于用户的主动靠近，即便开了「平时隐藏」也允许悬停展开。
        let idle_popup = self.settings.idle_mode == model::IdleMode::Hidden && self.near_top_now && !self.open;
        let hover_opens = (!self.settings.auto_hide || self.t < self.forced_open_until || idle_popup) && !self.dnd_now;
        // 长按快捷操作进行中（按下未松开，或菜单已弹出）时，不要同时把面板也展开，
        // 否则长按会既弹菜单又开面板，体验冲突。
        let gesture_busy = self.press_since.is_some() || self.quick_menu.is_some();
        // 鼠标明确停在岛体上时「绝不收起」——不看 hover_opens / 勿扰 / hover_lock，
        // 避免看到一半被收起。只有离开岛体后，才由设置里的「移开多久收起」决定是否收起。
        let stay_open = self.open && inside && !self.browser_minimal_active;
        let want = stay_open
            || (inside && hover_opens && !self.hover_lock && !gesture_busy && (self.compact_notices()||self.notice.is_none()||self.open))
            || self.open && on_notice
            || (self.open && self.t<self.forced_open_until)
            || self.drag_hot
            || (self.is_editing() && self.open);
        self.morph(want, inside, env.t, dt);
        // 「水滴」融入动画：先收成小药丸、弹簧落定后，再缓缓收成小水珠融入顶部。
        // 预览期间停在药丸态，方便用户在设置里看清选中的形态。
        let settled = self.w.pos < IDLE_PILL_W + 4.0 && self.h.pos < IDLE_PILL_H + 4.0;
        let drop_target = if !self.previewing() && self.idle_pill() && settled { 1.0 } else { 0.0 };
        self.idle_drop = anim::approach(self.idle_drop, drop_target, 2.6, dt);
        // 展开瞬间：播放「啵」一声。
        if self.open && !self.was_open && self.settings.sound && !self.dnd_now {
            sfx::play(sfx::Sfx::Boop);
        }
        self.was_open = self.open;
        self.update_pet(env, cx);

        // ---------- 绘制 ----------
        let p = self.progress();
        let (w, h) = (self.w.pos, self.h.pos);
        let panel0 = Rect::from_min_size(Pos2::new(cx - w / 2.0, top), Vec2::new(w, h));
        let panel = Rect::from_center_size(panel0.center(), panel0.size() * (if self.open { 1.0 } else { 1.0 + 0.06 * self.peek.pos }));
        let pc = anim::clamp01(p);
        let painter = ui.painter_at(ui.max_rect().expand(40.0));
        let feather = 1.0 / ctx.pixels_per_point();
        // 歌词 / 通知 / 铃铛期间不画成极简，保持内容可见；其余情况按 effective_minimal。
        let minimal_pill = self.effective_minimal()
            && !self.island_lyrics_active()
            && self.notice.is_none()
            && (self.previewing() || self.t >= self.notification_bell_until)
            && pc < 0.02;
        let flare = if minimal_pill { 0.0 } else { lerp(5.0, FLARE, pc) };
        let k = (w / PW).clamp(0.3, 1.1);
        let radius = if minimal_pill { (self.h.pos.max(4.0)) / 2.0 } else { lerp(12.5, RAD * k.min(1.0), pc) };
        let flare_h = lerp(8.0, FLARE_H, pc);
        let thick = glass::thickness(PH);
        let shadow_k = glass::shadow_alpha(0.0, thick) / 0.12;
        shape::shadow(&painter, panel, flare, flare_h, radius, smoothstep(0.1, 0.9, pc) * shadow_k);
        let notice_rect=self.notice_rect(cx,top);
        // 传统通知徽章的黑色衬底只画旧徽章；分裂小胶囊自己画胶囊外形。
        let notice_shape=if self.side_slot.is_none() { notice_rect.map(|r|Rect::from_min_max(Pos2::new(r.left()-36.0,r.top()),r.max)) } else { None };
        if let Some(r)=notice_shape {shape::shadow(&painter,r,5.0,8.0,12.5,0.5*shadow_k);}
        shape::fill(&painter, panel, flare, flare_h, radius, Color32::BLACK, feather);
        if let Some(r)=notice_shape {shape::fill(&painter,r,5.0,8.0,12.5,Color32::BLACK,feather);}

        // 内容只随形体「揭开」：不缩放，按面板裁剪，到 55% 才开始淡入
        let reveal = if self.open { smoothstep(0.50, 0.97, p) } else { smoothstep(0.55, 0.92, p) };
        self.reveal = reveal;
        let origin = Pos2::new(cx - PW / 2.0, top);
        let clip = panel.intersect(win);
        let base = Card::new(ui, origin, clip)
            .scaled(PW / 2.0, h / 2.0, if self.open { 1.0 } else { 1.0 + 0.06 * self.peek.pos });
        self.cursor_local = env.cursor.map(|p| base.to_local(p));

        let banner=!self.compact_notices()&&self.notice.is_some()&&!self.open;
        let restoring_lyrics=!self.open&&self.notice.is_none()&&self.island_lyrics_active()&&self.hud_notice.as_ref().is_some_and(|a|a.meeting.is_some());
        let fading_hud=!self.compact_notices()&&!self.open&&self.hud_notice.as_ref().is_some_and(|a|a.is_hud())&&(banner||self.hud_alpha>0.003);
        if fading_hud {
            let from=self.resident.target.width;let to=HUD_W+self.pet_space();
            let width_progress=if (to-from).abs()<1.0 {if banner{1.0}else{0.0}}else{((self.w.pos-from)/(to-from)).clamp(0.0,1.0)};
            let (opacity,header_opacity)=if banner {(self.hud_alpha*smoothstep(0.45,0.85,width_progress),1.0-smoothstep(0.05,0.40,width_progress))}
                else {(self.hud_alpha*smoothstep(0.20,0.60,width_progress),1.0-smoothstep(0.05,0.25,width_progress))};
            if !minimal_pill||pc>0.0 {self.shared_header(&base.faded(header_opacity),cx,pc,reveal,env);}
            if let Some(a)=self.hud_notice.clone(){
                let overlay=Card::new(ui,panel.min+Vec2::new(self.pet_space(),0.0),panel.intersect(ui.max_rect())).faded(opacity);
                let overlay=if opacity<0.85||!banner{overlay.ghost()}else{overlay};
                self.draw_notice(&overlay,panel,&a,&mut cmds);
            }
        }else if banner {
            if let Some(a)=self.notice.clone(){self.draw_notice(&Card::new(ui,panel.min+Vec2::new(self.pet_space(),0.0),panel.intersect(ui.max_rect())),panel,&a,&mut cmds);}
        }else if !minimal_pill || pc > 0.0 {
            self.shared_header(&base, cx, if restoring_lyrics{0.0}else{pc}, reveal, env);
        }
        if let Some(rect)=notice_rect {
            let drawn=Rect::from_center_size(rect.center(), rect.size() * (if self.open {1.0} else {1.0+0.06*self.peek.pos}));
            if let Some(a)=self.side_slot.clone() {
                // 分裂小胶囊：与主岛用 side_gap 弹簧拉开，呈现「第二活动」。
                shape::shadow(&painter,drawn,5.0,8.0,BAR_H/2.0,0.5*shadow_k);
                shape::fill(&painter,drawn,5.0,8.0,BAR_H/2.0,Color32::BLACK,feather);
                let c=Card::new(ui,drawn.min,drawn.intersect(ui.max_rect()));
                match self.draw_side_activity(&c,drawn,&a) {
                    crate::activity::SideClick::Page(p)=>self.show_page(p),
                    crate::activity::SideClick::Details=>self.notice_details=!self.notice_details,
                    crate::activity::SideClick::None=>{}
                }
            } else if let Some(a)=self.hud_notice.clone() {
                let c=Card::new(ui,drawn.min,drawn.intersect(ui.max_rect()));
                let opacity=self.hud_alpha*smoothstep(0.45,0.95,self.notice_width.pos/44.0);
                let c=c.faded(opacity);let c=if self.notice.is_none()||opacity<0.85 {c.ghost()}else{c};
                crate::extras::notice_badge(&c,18.0,15.0,&a);
                let button=crate::quota_ui::bar_tooltip(c.hit(0.0,0.0,36.0,BAR_H,"notice-badge"),format!("{}\n{}",a.title,a.body));
                if button.clicked(){self.notice_details=!self.notice_details;}
            }else if self.t<self.notification_bell_until {
                let c=Card::new(ui,drawn.min,drawn.intersect(ui.max_rect()));
                paint::bell(&c.scaled(20.0,14.0,1.2),20.0,14.0,theme::WHITE,paint::bell_swing(self.t-self.notification_bell_at));
                c.circle(24.0,9.5,2.0,Color32::from_rgb(255,72,72));
                if c.hit(6.0,0.0,28.0,30.0,"bell-mail").clicked(){self.show_page(5);}
            }
        }
        let notice_details=self.notice_details_rect(cx,top);
        if let Some(rect)=notice_details {
            let radius=if self.hud_notice.as_ref().is_some_and(|a|a.meeting.is_some()){14.0}else{10.0};
            shape::shadow(&painter,rect,0.0,0.0,radius,0.5*shadow_k);
            painter.rect_filled(rect,radius,Color32::BLACK);
            self.draw_notice(&Card::new(ui,rect.min,rect.intersect(ui.max_rect())),rect,&self.hud_notice.clone().unwrap(),&mut cmds);
            if ctx.input(|i|i.pointer.any_pressed()&&i.pointer.interact_pos().is_some_and(|p|!rect.contains(p)&&!notice_rect.is_some_and(|r|r.contains(p)))){self.notice_details=false;}
        }
        let pet_alpha=self.pet_a*(1.0-smoothstep(0.0,0.3,pc));
        if pet_alpha>0.004 && !minimal_pill {
            let y=(h.min(self.base_size().1)-BAR_H).max(0.0)/2.0;
            self.pet.draw(&base.shifted(0.0,y).faded(pet_alpha),cx-origin.x+self.resident.position(bar::PET),self.t);
        }
        if reveal > 0.004 && !banner && !restoring_lyrics {
            let c = base.shifted(0.0, (1.0 - reveal) * -5.0).faded(reveal);
            let c = if reveal < 0.6 { c.ghost() } else { c };
            if self.calendar.draft.is_some(){self.draw_calendar_editor(&c);}else if self.task_dialog.is_some(){self.draw_task_editor(&c);}else{self.panel_body(&c, env, &mut cmds);}
            if !self.toast_below {self.toast_layer(&c,106.0);}
        }
        // 岛下的确认提示使用独立裁剪区域，收起时跟随岛体下沿。
        let floating_toast=if self.toast_below && self.toast.is_some() {
            self.toast_layer(&Card::new(ui,origin,win),h+8.0)
        }else{None};
        if (self.adding||self.editing.is_some()) && reveal > 0.9 && self.page == 4 && self.page_e() > 0.25 {
            self.input_field(ui, origin, &mut cmds);
        }

        if self.task_dialog.is_some() && reveal>0.9 {self.task_editor_inputs(ui,origin,&mut cmds);}
        if self.calendar.draft.is_some() && reveal>0.9 {self.calendar_editor_inputs(ui,origin,&mut cmds);}

        // 长按胶囊弹出的快捷操作条。
        self.draw_quick_menu(ui, cx, top, &mut cmds);

        // 岛史：收起态下在胶囊右侧显示未读活动计数，点击展开历史面板。
        // 极简细线 / 空闲小药丸下不显示，避免小岛旁挂一个会超出裁剪区的小圆。
        if !self.open && !self.island_history.is_empty() && !self.effective_minimal() {
            let bx = cx + w / 2.0 + 6.0;
            let by = top + 2.0;
            let badge = Card::new(ui, Pos2::new(bx, by), ui.max_rect());
            if badge.hit(0.0, 0.0, 22.0, BAR_H - 4.0, "island-history").clicked() {
                self.history_open = !self.history_open;
            }
            painter.circle_filled(Pos2::new(bx + 11.0, by + (BAR_H - 4.0) / 2.0), 11.0, Color32::from_black_alpha(210));
            badge.centered_text(11.0, (BAR_H - 4.0) / 2.0 + 4.0, &format!("{}", self.island_history.len()), 11.0, W::Semibold, theme::WHITE, H::Center);
            if self.history_open {
                // 面板最多列 8 条，避免历史很长时拉出一个超高面板。
                let count = self.island_history.len().min(8);
                let row_h = 30.0;
                let list = Rect::from_min_size(Pos2::new(cx - 150.0, top + BAR_H + 8.0), Vec2::new(300.0, count as f32 * row_h + 8.0));
                shape::shadow(&painter, list, 0.0, 0.0, 12.0, 0.5);
                painter.rect_filled(list, 12.0, Color32::from_black_alpha(225));
                let entries: Vec<(usize, String)> = self.island_history.iter().take(8).map(|a| {
                    let label = if a.detail.is_empty() { a.title.clone() } else { format!("{} · {}", a.title, a.detail) };
                    (a.page, label)
                }).collect();
                for (i, (page, label)) in entries.into_iter().enumerate() {
                    let ry = top + BAR_H + 8.0 + 4.0 + i as f32 * row_h;
                    let row = Card::new(ui, Pos2::new(cx - 150.0, ry), ui.max_rect());
                    if row.hit(4.0, 0.0, 292.0, row_h, format!("history-row-{i}")).clicked() {
                        self.show_page(page);
                        self.history_open = false;
                    }
                    row.centered_text(12.0, row_h / 2.0 + 4.0, &label, 12.0, W::Semibold, theme::WHITE, H::Left);
                }
            }
        }

        // 夜间变暗：深夜把岛身压暗，不刺眼。
        if self.night > 0.003 {
            let a = (self.night * 0.45 * 255.0) as u8;
            let r = panel.union(notice_rect.unwrap_or(panel));
            painter.rect_filled(r, radius, Color32::from_black_alpha(a));
        }

        // ---------- 输出 ----------
        let rel = |r: Rect| r.translate(-win.min.to_vec2());
        // 「完全隐入」：空闲时窗口裁剪为空，看似彻底消失，实则可被鼠标贴顶温柔唤起。
        let idle_hidden = self.idle_hidden();
        let mut region = if idle_hidden {
            Rect::from_min_size(Pos2::new(cx, top), Vec2::ZERO)
        } else if p > 0.02 || self.open {
            let height=self.expanded_height().max(self.h.pos)*1.08+16.0;
            Rect::from_center_size(Pos2::new(cx, top + height/2.0), Vec2::new(PW + FLARE * 2.0 + 56.0, height))
        } else {
            let (bw, bh) = self.base_size();
            Rect::from_center_size(Pos2::new(cx, top + (bh * SWELL.1 + 12.0) / 2.0), Vec2::new(bw * SWELL.0 + 24.0, bh * SWELL.1 + 12.0))
        };
        if !idle_hidden {
            if let Some(rect)=notice_rect {region=region.union(rect.expand2(Vec2::new(12.0,8.0)));}
            if let Some(rect)=notice_shape {region=region.union(rect.expand2(Vec2::new(12.0,8.0)));}
            if let Some(rect)=notice_details {region=region.union(rect.expand(12.0));}
            // 长按快捷条 / 岛史面板画在胶囊下方，必须并入裁剪区，否则会被窗口区域切掉。
            if self.quick_menu.is_some() {
                region=region.union(Rect::from_min_size(Pos2::new(cx-144.0, top+BAR_H+8.0), Vec2::new(288.0, 28.0)).expand(8.0));
            }
            if self.history_open && !self.effective_minimal() {
                let h = self.island_history.len().min(8) as f32 * 30.0 + 16.0;
                region=region.union(Rect::from_min_size(Pos2::new(cx-150.0, top+BAR_H+8.0), Vec2::new(300.0, h)).expand(6.0));
            }
        }
        let transitioning = !self.w.settled(PW)
            || !self.h.settled(self.expanded_height())
            || self.page_e() < 0.7
            || self.dots.iter().any(|d| !d.settled(16.0))
            || self.hover_since.is_some() && !self.open
            || self.leave_since.is_some()
            || (self.drag_a > 0.001 && self.drag_a < 0.999)
            || self.drag_hot
            || (self.pet_a > 0.001 && self.pet_a < 0.999)
            || !self.resident.settled()
            || self.open && self.page==3 && self.shelf_x.values().any(|s| !s.settled(68.0))
            || self.open && self.page==2 && !self.water_ring.settled(1.0)
            || self.toast.is_some()
            || self.t < self.notification_bell_until && self.t-self.notification_bell_at < 0.95
            || self.hud_alpha > 0.003 && self.hud_alpha < 0.997
            || !self.notice_width.settled(1.0);
        let pet_visible=self.pet_a>0.01 && !self.open && !minimal_pill;
        let ambient=self.pomo.running && (self.open && self.page==1 || !self.open && !minimal_pill && self.notice.is_none() && bar::animations_enabled())
            || self.media.playing && (self.open && self.page==0 || !self.open && !minimal_pill && self.notice.is_none() && self.media.is_music())
            || self.notice.as_ref().is_some_and(|notice|notice.meeting.is_some());
        let animating=transitioning || ambient || pet_visible && self.pet.state!=PetState::Sleeping;
        // 交互形变保持 60 帧，媒体 30 帧；小表情 20 帧，睡眠 10 帧，静态界面每秒核对提醒。
        // 交互形变保持 60 帧，媒体 30 帧；小表情 20 帧，睡眠 10 帧，静态界面每秒核对提醒。
        // 「完全隐入」时窗口裁剪为空、收不到鼠标事件，靠钩子轮询近顶，因此按 10 帧/秒检查。
        let repaint_after=std::time::Duration::from_secs_f64(if idle_hidden{0.1}else if transitioning{1.0/60.0}else if ambient{1.0/30.0}else if pet_visible{if self.pet.state==PetState::Sleeping{0.1}else{0.05}}else{1.0});
        cmds.append(&mut self.pending);
        let zero = || rel(Rect::from_min_size(Pos2::new(cx, top), Vec2::ZERO));
        FrameOut {
            region: if idle_hidden { zero() } else { rel(floating_toast.map_or(region,|toast|region.union(toast)).intersect(ui.max_rect())) },
            hit: if idle_hidden { zero() } else { rel(self.hit_rect(cx, top, self.progress())) },
            notice_hit: if idle_hidden { None } else { notice_shape.map(|r|rel(notice_details.map_or(r,|details|r.union(details)))) },
            file_drop_zone: if idle_hidden { None } else { self.settings.visible_pages[3].then(||rel(Rect::from_min_size(Pos2::new(cx-PW/2.0,top),Vec2::new(PW,96.0)))) },
            expanded: self.open,
            editing: self.is_editing(),
            animating,
            repaint_after,
            cmds,
        }
    }

    fn hit_rect(&self, cx: f32, top: f32, p: f32) -> Rect {
        if self.effective_minimal() && !self.open {
            let (width,height)=self.base_size();
            // 小药丸给足够大的命中区，方便点击展开；细线仍保持原有很薄的命中区，
            // 避免挡住浏览器标签栏（这是之前修过的回归点）。
            let (min_w,min_h)=if self.idle_pill(){(120.0,26.0)}else{(0.0,0.0)};
            let width=self.w.pos.min(width).max(min_w);
            let height=self.h.pos.min(height).max(min_h);
            Rect::from_min_size(Pos2::new(cx-width/2.0,top),Vec2::new(width,height))
        } else if self.notice.is_some()&&!self.compact_notices()&&!self.open {
            let (w,h)=self.base_size();Rect::from_min_size(Pos2::new(cx-(w+8.0)/2.0,top),Vec2::new(w+8.0,h+6.0))
        } else if p > 0.04 || self.open {
            Rect::from_min_size(Pos2::new(cx - PW / 2.0 - 4.0, top), Vec2::new(PW + 8.0, self.expanded_height() + 2.0))
        } else {
            let (bw,bh)=self.base_size();
            let (hit_w,hit_h)=(bw+12.0,bh+4.0);
            Rect::from_min_size(Pos2::new(cx-hit_w/2.0,top),Vec2::new(hit_w,hit_h))
        }
    }

    fn notice_rect(&self,cx:f32,top:f32)->Option<Rect>{
        if !self.compact_notices() && self.side_slot.is_none(){return None;}
        if self.notice_width.pos<=0.5 {return None;}
        let gap=self.side_gap.pos;
        Some(Rect::from_min_size(Pos2::new(cx+self.w.pos/2.0+gap,top),Vec2::new(self.notice_width.pos,BAR_H)))
    }
    fn notice_details_rect(&self,cx:f32,top:f32)->Option<Rect>{
        let a=self.hud_notice.as_ref().filter(|_|self.notice_details&&self.notice.is_some())?;
        let badge=self.notice_rect(cx,top)?;let (width,height)=self.notice_size(a);
        Some(Rect::from_min_size(badge.left_bottom()+Vec2::new(0.0,6.0),Vec2::new(width,height)))
    }

    /// 悬停状态机：意图缓冲 → 弹簧展开 → 离开延迟 → 临界阻尼收起；全程可打断。
    fn morph(&mut self, want: bool, inside: bool, t: f64, dt: f32) {
        let (bw, bh) = self.base_size();
        if want {
            self.leave_since = None;
            let since = *self.hover_since.get_or_insert(t);
            let intent=if self.effective_minimal() {MINIMAL_HOVER_INTENT}else{HOVER_INTENT};
            if !self.open && (self.drag_hot || t - since >= intent) {
                self.open = true;
                self.w.tune(OPEN_W.0, OPEN_W.1);
                self.h.tune(OPEN_H.0, OPEN_H.1);
                self.w.target = PW;
                self.h.target = self.expanded_height();
                if self.drag_hot {
                    // 拖入不等缓冲，直接给一点初速度，30 ms 内就能看到形变
                    self.w.vel = self.w.vel.max(600.0);
                    self.h.vel = self.h.vel.max(260.0);
                }
            } else if !self.open {
                self.w.tune(SWELL_K.0, SWELL_K.1);
                self.h.tune(SWELL_K.0, SWELL_K.1);
                self.w.target = bw * SWELL.0;
                self.h.target = bh * SWELL.1;
            }
        } else {
            self.hover_since = None;
            if self.open {
                let since = *self.leave_since.get_or_insert(t);
                if t - since >= self.settings.leave_delay as f64 {
                    self.open = false;
                    self.leave_since = None;
                    self.w.tune(CLOSE.0, CLOSE.1);
                    self.h.tune(CLOSE.0, CLOSE.1);
                    self.w.target = bw;
                    self.h.target = bh;
                    self.cancel_edit();
                    self.confirm_clear = None;
                }
            } else {
                self.w.tune(CLOSE.0, CLOSE.1);
                self.h.tune(CLOSE.0, CLOSE.1);
                self.w.target = bw;
                self.h.target = bh;
            }
        }
        let _ = inside;
        if self.open {self.h.target=self.expanded_height();}
        let (prev_w,prev_h)=(self.w.pos,self.h.pos);
        self.w.step(dt);
        self.h.step(dt);
        // 仅在从外侧越过目标时收住，扩展提示条时不强制跳到目标宽度。
        if !self.open {
            if prev_w>=bw && self.w.pos<bw {self.w.snap(bw);}
            if prev_h>=bh && self.h.pos<bh {self.h.snap(bh);}
        }
        for d in &mut self.dots {
            d.step(dt);
        }
        self.slide.step(dt);
    }

    // ---------------------------------------------------------------
    // 顶栏：电池 + 日期 + 时钟。药丸里的电池与时钟会「飞」到顶栏位置（共享元素过渡）。
    // ---------------------------------------------------------------
    fn shared_header(&mut self, base: &Card, cx: f32, p: f32, reveal: f32, env: &Env) {
        let local_cx = cx - base.origin.x;
        let lyric=self.island_lyric();
        let lyric_alpha=if self.island_lyrics_active() {1.0-smoothstep(0.0,0.35,p)}else{0.0};
        if let Some(galley)=self.island_lyric_layout.clone().filter(|_|lyric_alpha>0.003) {
            let c=base.faded(lyric_alpha);
            let content_width=self.lyric_bar_width()-self.lyric_focus_space();
            let width=self.w.pos.max(content_width);
            let left=local_cx-width/2.0;
            let center_y=BAR_H/2.0;
            if let Some((image,texture))=&self.music_artwork {
                let [width,height]=image.size;
                let uv_size=if width>height {egui::vec2(height as f32/width as f32,1.0)}else{egui::vec2(1.0,width as f32/height as f32)};
                let uv=Rect::from_center_size(Pos2::new(0.5,0.5),uv_size);
                c.painter.add(egui::epaint::RectShape::filled(c.rect(left+12.0,center_y-10.0,20.0,20.0),5.0*c.s,Color32::WHITE).with_texture(texture.id(),uv));
            } else {
                c.fill(left+12.0,center_y-10.0,20.0,20.0,5.0,theme::CARD);
                paint::note(&c,left+22.0,center_y,13.0,theme::WHITE);
            }
            let index=lyric.map_or(0,|(index,_)|index);
            let shown=c.ui.ctx().animate_value_with_time(egui::Id::new(("island-lyric-line",&self.ly.key)),index as f32,0.18);
            let fade=1.0-(index as f32-shown).abs().min(1.0);
            let position=c.pos(left+content_width-LYRIC_BAR_W/2.0,center_y-galley.size().y/2.0+(1.0-fade)*3.0);
            c.painter.galley(position,galley,if lyric.is_some(){theme::WHITE}else{theme::GRAY});
            paint::playback_bars(&c,left+content_width-21.0,center_y,self.t,self.media.playing,theme::WHITE);
            if self.pomo.running {
                let alpha=smoothstep(0.88,1.0,(width-content_width)/self.lyric_focus_space());
                crate::focus_bar::draw(&c.faded(alpha),left+width-14.0-crate::focus_bar::LYRIC_WIDTH/2.0,center_y,self.t,&self.pomo,self.phase_secs(self.pomo.phase),true);
            }
        }
        if lyric_alpha>=0.997 {return;}
        let header=base.faded(1.0-lyric_alpha);
        let base=&header;
        let m = anim::ease_in_out(smoothstep(0.0, 0.85, p));
        let clock = env.clock.get(..5).unwrap_or("--:--").to_string();
        let show_battery=self.settings.show_battery && env.battery.is_some();

        // 顶栏终点布局：[电池][日期][时钟] 作为一组整体居中
        let date_w = base.text_w(&env.date, size::DATE, W::Regular);
        let clock_w = base.text_w(&clock, size::CLOCK, W::Bold);
        let battery_space=if show_battery {24.0}else{0.0};
        let group = battery_space + date_w + 8.0 + clock_w;
        let gx = local_cx - group / 2.0 + if show_battery {6.0}else{0.0};
        let hb = (gx + 8.5, 18.6);
        let hd = gx + battery_space;
        let hc = hd + date_w + 8.0;
        // 药丸起点布局
        let has_music=self.media.is_music();
        let layout=self.resident.current();
        let pb=(local_cx+layout.x[bar::BATTERY].unwrap_or(0.0),15.0);
        let pcx=local_cx+layout.x[bar::CLOCK].unwrap()-base.text_w(&clock,size::PILL_CLOCK,W::Bold)/2.0;

        if self.effective_minimal() && self.t>=self.notification_bell_until && p < 0.02 { return; }
        // 电池：位置/大小连续插值
        let bk = lerp(1.0, 0.8, m);
        let bpos = (lerp(pb.0, hb.0, m), lerp(pb.1, hb.1, m));
        if show_battery {paint::battery(&base.faded(lerp(layout.clearance(bar::BATTERY),1.0,m)), bpos.0, bpos.1, env.battery, env.charging, bk);}

        // 时钟：两个字号沿同一路径交叉淡化，避免逐帧换字号
        let cpos = (lerp(pcx, hc, m), lerp(21.0, 23.0, m));
        let a_pill = (1.0 - smoothstep(0.25, 0.6, m)) * layout.clearance(bar::CLOCK);
        let a_head = smoothstep(0.35, 0.75, m);
        if a_pill > 0.0 {
            base.faded(a_pill).text(cpos.0, cpos.1, &clock, size::PILL_CLOCK, W::Bold, theme::WHITE, H::Left);
        }
        if a_head > 0.0 {
            base.faded(a_head).text(cpos.0, cpos.1 - 0.5, &clock, size::CLOCK, W::Bold, theme::WHITE, H::Left);
        }
        // 日期随内容淡入，从药丸方向轻轻滑出
        if reveal > 0.0 {
            base.faded(reveal).text(hd + (1.0 - reveal) * 10.0, 22.5, &env.date, size::DATE, W::Regular, Color32::from_rgb(210, 210, 214), H::Left);
        }
        if has_music && (p>0.1 || layout.x[bar::MUSIC].is_some()) {
            paint::playback_bars(base,lerp(local_cx+layout.x[bar::MUSIC].unwrap_or(0.0),367.0,m),lerp(15.0,18.0,m),self.t,self.media.playing,theme::WHITE);
        }
        let compact_alpha=1.0-smoothstep(0.0,0.22,p);
        if self.t<self.notification_bell_until&&!self.compact_notices()&&p<0.1 {
            if let Some(x)=layout.x[bar::BELL]{
                let x=local_cx+x;paint::bell(&base.scaled(x,14.0,1.2),x,14.0,theme::WHITE,paint::bell_swing(self.t-self.notification_bell_at));
                base.circle(x+4.0,9.5,2.0,Color32::from_rgb(255,72,72));
                if base.hit(x-14.0,3.0,28.0,25.0,"bell-mail").clicked(){self.show_page(5);}
            }
        }
        if compact_alpha>0.004 {
            let c=base.faded(compact_alpha);let c=if compact_alpha<0.8{c.ghost()}else{c};
            self.draw_quota_bar(&c,local_cx,&layout);
        }
        if self.pomo.running && compact_alpha>0.004 {
            crate::focus_bar::draw(&base.faded(compact_alpha),local_cx+layout.x[bar::FOCUS].unwrap(),15.0,self.t,&self.pomo,self.phase_secs(self.pomo.phase),false);
        }
    }

    fn update_pet(&mut self, env: &Env, cx: f32) {
        let cx=cx+self.resident.position(bar::PET);
        let idle = self.idle_for();
        if self.pet.last_idle >= self.opts.pet_idle && idle < 1.0
            && matches!(self.pet.state, PetState::Dozing | PetState::Sleeping) {
            self.pet.waking_until = self.t + 2.2;
        }
        self.pet.last_idle = idle;
        self.pet.hovered = env.cursor.is_some_and(|p| (p.x-cx).abs() <= BAR_W/2.0+24.0 && (0.0..70.0).contains(&p.y));
        self.pet.worried = self.settings.show_battery && env.battery.is_some_and(|v|v<=20) && !env.charging || env.network == Some(false);
        let allowed = self.pet_enabled();
        if !allowed { self.pet.reaction = None; self.pet.waking_until = 0.0; }
        let available = allowed && !self.open && !self.drag_hot;
        let shown = available;
        self.pet_a = anim::approach(self.pet_a, if shown {1.0}else{0.0}, 9.0, self.dt);
        let visible = available && self.pet_a > 0.7 && self.progress() < 0.1;
        self.pet.advance(self.dt, visible);
        let state = self.pet_state();
        let gaze = env.cursor.map(|p| [((p.x-cx)/28.0).clamp(-3.0,3.0), ((p.y-15.0)/40.0).clamp(-1.0,1.5)])
            .unwrap_or([(self.t as f32 * 0.35).sin()*0.8, 0.0]);
        self.pet.mouth_enabled = self.settings.pet_mouth;
        self.pet.eye_speed = self.settings.pet_eye_speed;
        self.pet.step(state, self.t, self.dt, gaze, visible);
    }

    // ---------------------------------------------------------------
    // 面板主体：页标题 + 页面（带切页过渡）+ 翻页箭头 + 页码豆
    // ---------------------------------------------------------------
    fn panel_body(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        let settings=c.hit(380.0,4.0,28.0,28.0,"page-settings").on_hover_text(format!("打开{}设置窗口",PAGE_NAMES[self.page]));
        let (hover,press)=c.hover_t(&settings);
        c.circle(394.0,18.0,13.0,Color32::from_white_alpha((hover*24.0) as u8));
        paint::gear(&c.scaled(394.0,18.0,0.8*(1.0-press*0.08)),394.0,18.0,paint::mix(theme::GRAY,theme::WHITE,hover));
        if settings.clicked(){self.settings_tab=[1,2,3,5,4,7,11,8][self.page];self.open_settings();}
        if self.subpage==Some(self.page) {
            c.text(15.0,25.0,"‹",19.0,W::Bold,theme::WHITE,H::Left);
            c.text(37.0,25.0,if self.page==2{"饮水设置"}else{"专注设置"},size::TITLE,W::Bold,theme::WHITE,H::Left);
            if c.hit(9.0,7.0,28.0,23.0,"subpage-back").clicked(){self.subpage=None;}
            self.page_settings(c);self.nav(c);return;
        }
        let e = self.page_e();
        let in_tx = e < 0.55 && self.page_from != self.page;
        if in_tx {
            // 旧页：原地向反方向轻移并快速淡出；新页：弹簧滑入 + 稍晚淡入
            let old_a = 1.0 - smoothstep(0.0, 0.16, e);
            let new_a = smoothstep(0.03, 0.28, e);
            let old_dx = -self.page_dir * 18.0 * anim::ease_out_cubic(e / 0.3);
            if old_a > 0.0 {
                let oc = c.faded(old_a).ghost();
                self.page_title(self.page_from, &oc);
                self.render_page(self.page_from, &oc.shifted(old_dx, 0.0), env, &mut Vec::new());
            }
            let nc = c.faded(new_a);
            self.page_title(self.page, &nc);
            let nc = nc.shifted(self.slide.pos, 0.0);
            let nc = if e < 0.12 { nc.ghost() } else { nc };
            self.render_page(self.page, &nc, env, cmds);
        } else {
            self.page_title(self.page, c);
            self.render_page(self.page, c, env, cmds);
        }
        self.nav(c);
    }

    fn page_title(&self, page: usize, c: &Card) {
        let base = 25.0;
        let (a, b, col) = match page {
            0 | CALENDAR_PAGE => return,
            1 => ("番茄", "专注", theme::RED_TOP),
            2 => ("饮水", "提醒", theme::accent()),
            3 => ("置物架", "", theme::WHITE),
            4 => ("待办", "清单", theme::GREEN),
            5 => ("邮箱", "", theme::CYAN),
            6 => ("AI", "额度", theme::SOFT_GREEN),
            _ => ("灵岛", "设置", theme::WHITE),
        };
        let wa = c.text(15.0, base, a, size::TITLE, W::Bold, theme::WHITE, H::Left);
        if !b.is_empty() {
            c.text(15.0 + wa + 1.5, base, b, size::TITLE, W::Bold, col, H::Left);
        }
    }

    fn nav(&mut self, c: &Card) {
        let pages: Vec<_>=self.settings.page_order.iter().copied().filter(|page|self.settings.visible_pages[*page]).collect();
        if pages.len()<2 { return; }
        let l = c.hit(8.0, 55.0, 26.0, 26.0, "nav-l");
        let r = c.hit(385.0, 55.0, 26.0, 26.0, "nav-r");
        let (lh, lp) = c.hover_t(&l);
        let (rh, rp) = c.hover_t(&r);
        paint::chevron(c, 21.0, 68.0, -1.0, lh, lp);
        paint::chevron(c, 398.0, 68.0, 1.0, rh, rp);
        if l.clicked() { self.navigate_page(false); }
        if r.clicked() { self.navigate_page(true); }
        // 页码豆：宽度由弹簧驱动，整行实时居中；当前页是白色长条
        let gap = 4.0;
        let dot_y=self.expanded_height()-7.2;
        let total: f32 = pages.iter().map(|page| self.dots[*page].pos).sum::<f32>() + gap * (pages.len()-1) as f32;
        let mut x = PW / 2.0 - total / 2.0;
        let mut clicked = None;
        for i in pages {
            let w = self.dots[i].pos.max(3.0);
            let on = anim::remap(w, 4.0, 16.0);
            let col = paint::mix(Color32::from_rgb(92, 92, 96), theme::WHITE, on);
            c.fill(x, dot_y - 2.0, w, 4.0, 2.0, col);
            let hr = c.hit(x - 2.0, dot_y - 6.0, w + 4.0, 12.0, ("dot", i));
            hr.clone().on_hover_text(PAGE_NAMES[i]);
            if hr.hovered() && i != self.page {
                c.fill(x - 1.0, dot_y - 3.0, w + 2.0, 6.0, 3.0, Color32::from_white_alpha(28));
            }
            if hr.clicked() {
                clicked = Some(i);
            }
            x += w + gap;
        }
        if let Some(i) = clicked {
            let position=|page|self.settings.page_order.iter().position(|p|*p==page).unwrap();
            let dir = if position(i) > position(self.page) { 1.0 } else { -1.0 };
            self.go(i, dir);
        }
    }

    fn render_page(&mut self, page: usize, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
        match page {
            0 => self.page_music(c, env, cmds),
            1 => self.page_focus(c, env),
            2 => self.page_water(c, env),
            3 => self.page_shelf(c, env, cmds),
            4 => self.page_todo(c, env, cmds),
            5 => self.page_mail(c, env, cmds),
            6 => self.page_quota(c),
            CALENDAR_PAGE => self.page_calendar(c,env),
            _ => self.page_settings(c),
        }
    }

    fn toast_layer(&mut self, c: &Card, base_y:f32)->Option<Rect> {
        let Some((msg, at)) = self.toast.clone() else { return None };
        let e = (self.t - at) as f32;
        if e > 2.4 {
            self.toast = None;
            return None;
        }
        let a = smoothstep(0.0, 0.16, e) * (1.0 - smoothstep(2.1, 2.4, e));
        let rise = (1.0 - anim::ease_out_cubic(e / 0.3)) * 6.0;
        let msg=c.fit(&msg,size::BODY,W::Regular,310.0);
        let w = c.text_w(&msg, size::BODY, W::Regular) + 26.0;
        let tc = c.faded(a);
        let y = base_y + rise;
        tc.fill(PW / 2.0 - w / 2.0, y, w, 22.0, 11.0, Color32::from_rgb(44, 44, 48));
        tc.text(PW / 2.0, y + 15.0, &msg, size::BODY, W::Regular, theme::WHITE, H::Center);
        Some(tc.rect(PW/2.0-w/2.0,y,w,22.0))
    }

    // ---------------------------------------------------------------
    // 每帧推进：番茄钟、饮水提醒、跨天
    // ---------------------------------------------------------------
    fn tick(&mut self, env: &Env, dt: f32, elapsed: f32) {
        if self.toast.as_ref().is_some_and(|(_,at)|self.t-at>2.4){self.toast=None;}
        if env.today != self.today {
            self.today = env.today.clone();
            self.water = model::Water::load(&env.today);
            self.last_drink=None;self.last_reminder=self.t;
            self.water_ml = self.water.drunk_ml as f32;
            self.water_ring.snap(self.water.percent() / 100.0);
        }
        // 久坐：只要键鼠一直有动静（空闲时间短）就累计，到阈值提醒一次再重新计时。
        if self.settings.sit_remind && env.idle_seconds.unwrap_or(0.0) < 90.0 {
            if self.sit_since <= 0.0 {
                self.sit_since = env.t;
            } else if env.t - self.sit_since >= self.settings.sit_minutes as f64 * 60.0 {
                self.sit_since = env.t;
                let minutes = self.settings.sit_minutes;
                self.alert("起来活动一下", format!("连续坐了 {minutes} 分钟，走两步再继续"), 1, true);
            }
        } else {
            self.sit_since = 0.0;
        }
        if self.pomo.running {
            self.pomo.remaining -= elapsed;
            if self.pomo.remaining <= 0.0 {
                let next = match self.pomo.phase {
                    Phase::Focus => {
                        // 一次专注完成，记到历史里；失败也不影响计时。
                        self.pomo_log.push(&env.today);
                        self.pet.react(PetState::Happy);
                        Phase::Rest
                    }
                    Phase::Rest => Phase::Focus,
                };
                self.pomo.phase = next;
                self.pomo.remaining = self.phase_secs(next);
                self.pomo.running = false;
                self.alert("专注计时", format!("该{}了", next.label()), 1, true);
            }
        }
        self.ring_shown.target = (self.pomo.remaining / self.phase_secs(self.pomo.phase)).clamp(0.0, 1.0);
        self.ring_shown.step(dt);
        self.reset_spin.step(dt);
        self.todo_pop.step(dt);

        if self.water.remind && self.water.percent() < 100.0 && self.water_hours(env.hour() as u32) && self.t - self.last_reminder >= self.settings.water_minutes as f64 * 60.0 {
            self.last_reminder = self.t;
            self.alert("喝水时间到了", format!("休息一下，喝 {} ml 水吧", self.settings.serving_ml), 2, true);
        }
        let now = format!("{} {}", env.today, env.clock.get(..5).unwrap_or("00:00"));
        let due: Vec<_> = self.todos.items.iter().enumerate().filter(|(_, item)| item.done_on.is_none() && !item.reminded && item.due.as_ref().is_some_and(|d| d <= &now)).map(|(i,item)| (i,item.text.clone())).collect();
        for (i,text) in due { self.todos.items[i].reminded = true; self.todos.save(); self.alert("待办到时间了", text, 4, true); }
        // v0.5.5 会议提醒：.ics 里的开始时间，默认提前 15 分钟（可设 5/15/30）。
        // v0.5.6 起日历（CalDAV）事件走同一套：同一场会（UID + DTSTART）只提醒一次，
        // 邮件和日历两条来源撞上了也只弹一次。
        let lead = self.settings.mail_ics as i64 * 60;
        let now_ts = chrono::Local::now().timestamp();
        let mut picks = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for entry in &self.calendar.local {
            let Some(at)=entry.reminder_at() else{continue;};
            let key=calendar::dedupe_key(&entry.event);
            let cutoff=if entry.event.all_day{at+60}else{entry.event.start+60};
            if now_ts>=at && now_ts<cutoff && !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                picks.push((key,entry.event.clone(),"本机日程".into(),CALENDAR_PAGE));
            }
        }
        for (m,provider) in self.calendar_work_events(None) {
            let left = m.start - now_ts;
            let key = calendar::dedupe_key(m);
            if !m.all_day && left <= lead && left > -60 && !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                let source=provider.map(|provider|provider.label()).unwrap_or("日历");
                picks.push((key, m.clone(), source.to_string(), CALENDAR_PAGE));
            }
        }
        if self.settings.mail_enabled {
            for m in self.mail.recent.iter().filter_map(|m| m.meeting.as_ref()).filter(|m|!self.mail_event_ignored(m)) {
                let left = m.start - now_ts;
                let key = calendar::dedupe_key(m);
                if !m.all_day && left <= lead && left > -60 && !self.mail.fired.contains(&key) && seen.insert(key.clone()) {
                    picks.push((key, m.clone(), "邮件邀请".to_string(), CALENDAR_PAGE));
                }
            }
        }
        for (key, meeting, source, page) in picks {
            self.mail.fired.insert(key);
            self.meeting_alert(meeting, source, page);
        }
        self.water_ring.target = self.water.percent() / 100.0;
        self.water_ring.step(dt);
        self.water_ml = anim::approach(self.water_ml, self.water.drunk_ml as f32, 9.0, dt);
        self.water_view_t.target = self.water_view as f32;
        self.water_view_t.step(dt);
        self.remind_t.target = if self.water.remind { 1.0 } else { 0.0 };
        self.remind_t.step(dt);
        self.shelf_scroll.target = self.shelf_first as f32;
        self.shelf_scroll.step(dt);
        if let Some(at) = self.confirm_clear {
            if self.t - at > 3.0 {
                self.confirm_clear = None;
            }
        }
    }

    /// 媒体进度在两次 SMTC 刷新之间按时间外推，进度条才会连续走动。
    fn update_media(&mut self, env: &Env, ctx: &egui::Context) {
        let m = &env.media;
        if !m.is_music() {
            self.media=NowPlaying::default();
            self.media_anchor=(String::new(),0.0,0.0,false);
            self.music_artwork=None;
            self.ly=LyricsState::default();
            return;
        }
        let a = &self.media_anchor;
        if a.0 != m.title || (a.1 - m.position).abs() > 0.01 || a.3 != m.playing {
            self.media_anchor = (m.title.clone(), m.position, env.t, m.playing);
        }
        let mut np = m.clone();
        if m.playing {
            let (_, pos, at, _) = &self.media_anchor;
            np.position = if let Some(sampled)=m.sampled_at {m.position+sampled.elapsed().as_secs_f64()}
                else {pos + (env.t - at).min(3.0)};
            if m.duration>0.0 {np.position=np.position.min(m.duration);}
        }
        if self.media.source != np.source || self.media.song_id != np.song_id { self.ly=LyricsState::default(); }
        self.media = np;
        if let Some(image)=&self.media.artwork {
            if !self.music_artwork.as_ref().is_some_and(|(old,_)|std::sync::Arc::ptr_eq(old,image)) {
                self.music_artwork=Some((image.clone(),ctx.load_texture("music-artwork",image.clone(),egui::TextureOptions::LINEAR)));
            }
        } else {self.music_artwork=None;}
    }

    fn poll_lyrics(&mut self) {
        if !self.media.is_music(){self.ly=LyricsState::default();return;}
        if !self.show_lyrics && !self.settings.island_lyrics {self.ly=LyricsState::default();return;}
        let np = &self.media;
        if np.title.is_empty(){self.ly=LyricsState::default();return;}
        let key = format!("{}|{}|{:.0}", np.title, np.artist, np.duration);
        if key != self.ly.key || self.ly.retry_at.is_some_and(|at|self.t>=at) {
            let native_only=key==self.ly.key && np.source.to_ascii_lowercase().contains("kugou")
                && self.ly.error==Some(lyrics::FetchError::NotFound);
            let cached=if key==self.ly.key && np.source.to_ascii_lowercase().contains("kugou"){self.ly.lyrics.take()}else{None};
            self.ly = LyricsState { key, loading: cached.is_none() && !native_only, lyrics:cached.clone(),
                error:native_only.then_some(lyrics::FetchError::NotFound), ..Default::default() };
            let (tx, rx) = std::sync::mpsc::channel();
            self.ly.rx = Some(rx);
            let (source, title, artist, dur) = (np.source.clone(), np.title.clone(), np.artist.clone(), np.duration);
            let song_id=np.song_id;
            if source.to_ascii_lowercase().contains("kugou") || lyrics::is_soda(&source) {
                let (tx,rx)=std::sync::mpsc::channel();self.ly.native_rx=Some(rx);
                let (source,title,artist)=(source.clone(),title.clone(),artist.clone());
                std::thread::spawn(move ||loop {
                    let found=lyrics::read_native(&source,&title,&artist,dur);
                    let done=found.is_some();
                    if tx.send(found).is_err() || done {break;}
                    std::thread::sleep(std::time::Duration::from_millis(500));
                });
            }
            std::thread::spawn(move || {
                let result=if native_only {
                    lyrics::kugou_cache::read(&title,&artist,dur).map(|lyrics|(lyrics,false)).ok_or(lyrics::FetchError::NotFound)
                }else{lyrics::fetch_for_display(&source, &title, &artist, dur, song_id, cached)};
                let _ = tx.send(result);
            });
        }
        if let Some(rx)=&self.ly.native_rx {
            let mut found=None;
            while let Ok(result)=rx.try_recv(){if result.is_some(){found=result;}}
            if let Some(lyrics)=found {
                self.ly.lyrics=Some(lyrics);self.ly.loading=false;self.ly.error=None;
                self.ly.retry_at=None;self.ly.rx=None;self.ly.native_rx=None;
            }
        }
        if let Some(rx) = &self.ly.rx {
            if let Ok(r) = rx.try_recv() {
                self.ly.loading = false;
                match r {
                    Ok((l,waiting_native)) => {
                        self.ly.error=None;
                        self.ly.retry_at=None;
                        if waiting_native || lyrics::is_soda(&self.media.source) && !l.synced {self.ly.retry_at=Some(self.t+5.0);}
                        self.ly.lyrics = Some(l);
                    },
                    Err(error) => {
                        if lyrics::is_soda(&self.media.source) || self.media.source.to_ascii_lowercase().contains("kugou") && error==lyrics::FetchError::NotFound {self.ly.retry_at=Some(self.t+5.0);}
                        else if matches!(error,lyrics::FetchError::Unavailable(_)|lyrics::FetchError::InvalidResponse(_)){self.ly.retry_at=Some(self.t+30.0);}
                        self.ly.error = Some(error);
                    },
                }
                self.ly.rx = None;
            }
        }
    }

    /// 外部直接提供歌词（录制演示用）。
    pub fn set_lyrics(&mut self, title: &str, artist: &str, duration: f64, l: lyrics::Lyrics) {
        self.ly = LyricsState { key: format!("{title}|{artist}|{duration:.0}"), lyrics: Some(l), ..Default::default() };
    }

    pub fn on_drop(&mut self, files: Vec<PathBuf>) {
        if !self.settings.visible_pages[3] || files.is_empty() { return; }
        self.drag_hot = false;
        self.drag_saved = None;
        if self.page != 3 {
            self.go(3, 1.0);
        }
        let (mut added,mut duplicate,mut failed) = (0,0,0);
        for p in files {
            if !p.exists() {failed+=1;continue;}
            if self.shelf.items.contains(&p) {
                duplicate+=1;
                continue;
            }
            if self.shelf.add(&p).is_ok() {
                self.shelf_born.insert(p.clone(), self.t);
                added += 1;
            }else{failed+=1;}
        }
        let message=if added==0 && failed==0 {
            if duplicate==1{"该文件已在置物架中".into()}else{"这些文件已在置物架中".into()}
        }else if added==0 && duplicate==0 {"文件不存在或无法访问".into()}
        else {
            let mut parts=Vec::new();
            if added>0{parts.push(format!("已添加 {added} 个文件"));}
            if duplicate>0{parts.push(format!("{duplicate} 个已存在"));}
            if failed>0{parts.push(format!("{failed} 个无法添加"));}
            parts.join("，")
        };
        self.say(message);
        if added>0 {
            self.pet.react(PetState::Happy);
            self.shelf_first = self.shelf.items.len().saturating_sub(5);
        }
    }

    /// 待办输入框：真正的 egui 文本框，放在待办卡片顶部那一行。
    fn input_field(&mut self, ui: &mut egui::Ui, origin: Pos2, cmds: &mut Vec<Cmd>) {
        let rect = Rect::from_min_size(origin + Vec2::new(222.0, 37.0), Vec2::new(142.0, 18.0));
        let mut buf = std::mem::take(&mut self.buf);
        let te = egui::TextEdit::singleline(&mut buf)
            .id(egui::Id::new("todo-input"))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .text_color(theme::WHITE)
            .font(theme::font(size::BODY, W::Regular))
            .hint_text(egui::RichText::new("要做点什么？回车保存").color(theme::DIM).font(theme::font(size::BODY, W::Regular)))
            .desired_width(rect.width());
        let resp = ui.put(rect, te);
        let first_focus=!self.todo_input_focused;
        self.buf=buf;
        let lost_window=ui.input(|i|i.events.iter().any(|event|matches!(event,egui::Event::WindowFocused(false))));
        if !first_focus && self.adding && self.buf.trim().is_empty() && (resp.clicked_elsewhere() || lost_window) {
            resp.surrender_focus();
            self.cancel_edit();
            return;
        }
        if first_focus {
            self.todo_input_focused=true;
            resp.request_focus();
            cmds.push(Cmd::Focus);
        }
        let submit = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if submit {
            let text = self.buf.trim().to_string();
            let mut added=false;
            if let Some(i) = self.editing {
                if !text.is_empty() && i < self.todos.items.len() {
                    self.todos.items[i].text = text;
                    self.todos.save();
                }
            } else if !text.is_empty() {
                self.todo_first = 0;
                self.todos.items.insert(0, model::TodoItem { text, done_on: None, due: None, reminded: false });
                self.todos.save();
                self.todo_pop.vel += 6.0;
                added=true;
            }
            self.cancel_edit();
            if added {self.open_task_reminder(0);}
        }
    }
}

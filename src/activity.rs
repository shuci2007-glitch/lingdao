//! 统一活动卡协议：验证码、日程、待办、喝水、番茄钟、正在播放
//! 统一描述成「活动」，按优先级排队；主岛身表达最高的一条，
//! 其余中最高的一条进入右侧分裂小胶囊——对齐灵动岛的双活动形态。
use crate::app::{App, Phase, BAR_H};
use crate::paint::{self, Card, H};
use crate::theme::{self, W};
use eframe::egui;
use eframe::egui::{Color32, Rect};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind {
    /// 邮件验证码
    Code,
    /// 日程 / 会议提醒
    Meeting,
    /// 待办到期
    Todo,
    /// 喝水提醒
    Water,
    /// 番茄钟
    Focus,
    /// 正在播放
    Media,
}

#[derive(Clone)]
pub(crate) struct Activity {
    pub kind: Kind,
    /// 越小越优先：验证码 > 日程 > 待办 > 喝水 > 专注 > 媒体。
    pub rank: u8,
    pub title: String,
    pub detail: String,
    /// 点击小胶囊跳转的页面。
    pub page: usize,
    /// 常驻型（专注/媒体）跟随状态存在；瞬时型来自通知队列。
    pub sticky: bool,
    /// 瞬时型对应的原始通知，用于小胶囊图标与详情弹层。
    pub alert: Option<crate::extras::Alert>,
}

impl Activity {
    /// 分裂小胶囊的目标总宽（含与主岛之间 8px 间隙的预算）。
    pub(crate) fn pill_width(&self) -> f32 {
        match self.kind {
            Kind::Code => 96.0,
            Kind::Focus => 78.0,
            Kind::Media => 46.0,
            _ => 46.0,
        }
    }
}

/// 分裂小胶囊的点击结果。
pub(crate) enum SideClick {
    Details,
    Page(usize),
    None,
}

impl App {
    /// 收集当前所有活动并按优先级排序。
    pub(crate) fn collect_activities(&self) -> Vec<Activity> {
        let mut v: Vec<Activity> = Vec::new();
        if let Some((code, at)) = &self.code_seen {
            if self.t - at < 600.0 {
                v.push(Activity {
                    kind: Kind::Code,
                    rank: 0,
                    title: "验证码".into(),
                    detail: code.clone(),
                    page: 5,
                    sticky: false,
                    alert: None,
                });
            }
        }
        if let Some(a) = &self.notice {
            let kind = match a.title.as_str() {
                "会议提醒" => Some(Kind::Meeting),
                "待办到时间了" => Some(Kind::Todo),
                "喝水时间到了" => Some(Kind::Water),
                _ => None,
            };
            if let Some(kind) = kind {
                let rank = match kind {
                    Kind::Meeting => 1,
                    Kind::Todo => 2,
                    _ => 3,
                };
                v.push(Activity {
                    kind,
                    rank,
                    title: a.title.clone(),
                    detail: a.body.clone(),
                    page: a.page,
                    sticky: false,
                    alert: Some(a.clone()),
                });
            }
        }
        if self.pomo.running {
            v.push(Activity {
                kind: Kind::Focus,
                rank: 4,
                title: if self.pomo.phase == Phase::Focus { "专注中" } else { "休息中" }.into(),
                detail: String::new(),
                page: 1,
                sticky: true,
                alert: None,
            });
        }
        if self.media.playing {
            v.push(Activity {
                kind: Kind::Media,
                rank: 5,
                title: self.media.title.clone(),
                detail: self.media.artist.clone(),
                page: 0,
                sticky: true,
                alert: None,
            });
        }
        v.sort_by_key(|a| a.rank);
        v
    }

    /// 主槽位（通知横幅 / 歌词岛 / 常驻栏）已表达的活动之外，
    /// 进入右侧分裂小胶囊的那一条。
    pub(crate) fn side_activity(&self, activities: &[Activity]) -> Option<Activity> {
        if self.open {
            return None;
        }
        if self.compact_notices() {
            // 歌词岛占主位：瞬时活动进小胶囊（会议横幅已被 compact 排除）。
            activities.iter().find(|a| !a.sticky).cloned()
        } else if self.notice.as_ref().is_some_and(|a| self.notice_size(a).1 <= 50.0) {
            // 通知横幅占主位：常驻活动（专注/媒体）进小胶囊，双活动并存。
            activities.iter().find(|a| a.sticky).cloned()
        } else {
            // 常驻栏模式：专注与媒体本就内嵌在岛里，无需分裂。
            None
        }
    }

    /// 绘制分裂小胶囊里的活动并返回点击意图。
    pub(crate) fn draw_side_activity(&mut self, c: &Card, rect: Rect, a: &Activity) -> SideClick {
        let width = rect.width();
        let cx = width / 2.0;
        let cy = BAR_H / 2.0;
        let mut click = SideClick::None;
        match a.kind {
            Kind::Focus => {
                crate::focus_bar::draw(c, cx, cy, self.t, &self.pomo, self.phase_secs(self.pomo.phase), true);
                if c.hit(0.0, 0.0, width, BAR_H, "side-focus").clicked() {
                    click = SideClick::Page(a.page);
                }
            }
            Kind::Media => {
                let mut art_drawn = false;
                if let Some((image, texture)) = &self.music_artwork {
                    let [iw, ih] = image.size;
                    let uv_size = if iw > ih {
                        egui::vec2(ih as f32 / iw as f32, 1.0)
                    } else {
                        egui::vec2(1.0, iw as f32 / ih as f32)
                    };
                    let uv = Rect::from_center_size(egui::Pos2::new(0.5, 0.5), uv_size);
                    c.painter.add(
                        egui::epaint::RectShape::filled(c.rect(6.0, cy - 10.0, 20.0, 20.0), 5.0 * c.s, Color32::WHITE)
                            .with_texture(texture.id(), uv),
                    );
                    art_drawn = true;
                }
                if !art_drawn {
                    c.fill(7.0, cy - 10.0, 20.0, 20.0, 5.0, theme::CARD);
                    paint::note(c, 17.0, cy, 13.0, theme::WHITE);
                }
                paint::playback_bars(c, width - 11.0, cy, self.t, self.media.playing, theme::WHITE);
                if c.hit(0.0, 0.0, width, BAR_H, "side-media").clicked() {
                    click = SideClick::Page(a.page);
                }
            }
            Kind::Code => {
                // 信封 + 验证码数字：验证码值得占据更宽的胶囊一眼可读。
                c.stroke(9.0, cy - 7.0, 18.0, 14.0, 3.0, 1.4, theme::accent());
                c.path(&[(10.0, cy - 5.0), (18.0, cy + 1.5), (26.0, cy - 5.0)], 1.4, theme::accent());
                c.centered_text(34.0, cy + 0.5, &a.detail, 12.5, W::Semibold, theme::WHITE, H::Left);
                if c.hit(0.0, 0.0, width, BAR_H, "side-code").clicked() {
                    click = SideClick::Page(a.page);
                }
            }
            _ => {
                if let Some(alert) = &a.alert {
                    crate::extras::notice_badge(c, cx, cy, alert);
                    if c.hit(0.0, 0.0, width, BAR_H, "side-notice").clicked() {
                        click = SideClick::Details;
                    }
                }
            }
        }
        let _ = &mut click;
        click
    }
}

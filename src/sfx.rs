//! 灵岛音效：弹出轻响、展开「啵」一声。
//! Windows 下用 PlaySoundW 播放内存里合成的小段 WAV，无需任何外部音频资源；
//! 其它平台留空实现，调用零成本。
use std::sync::OnceLock;

/// 音效种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    /// 新活动出现 / 岛身冒头：短促上扬的「叮」。
    Pop,
    /// 展开大面板 / 胶囊回弹：更圆润的「啵」。
    Boop,
}

#[cfg(windows)]
mod imp {
    use super::Sfx;
    use std::sync::OnceLock;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY};
    use windows::Win32::Foundation::HMODULE;
    use windows::core::PCWSTR;

    /// 采样率与单声道 16bit 合成。
    const SR: u32 = 44_100;

    /// 合成一段音并打包成 WAV（PCM 16bit 单声道）字节。
    fn make_wav(kind: Sfx) -> Vec<u8> {
        let total = match kind { Sfx::Pop => 0.16, Sfx::Boop => 0.22 };
        let n = (SR as f32 * total) as usize;
        let mut samples: Vec<i16> = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / SR as f32;
            let env = match kind {
                Sfx::Pop => (1.0 - t / total).max(0.0).powf(2.2),
                Sfx::Boop => (std::f32::consts::PI * t / total).sin().powf(0.7),
            };
            let f0 = if kind == Sfx::Pop { 880.0 } else { 520.0 };
            let f1 = if kind == Sfx::Pop { 1320.0 } else { 360.0 };
            let freq = f0 + (f1 - f0) * (t / total);
            let wave = (2.0 * std::f32::consts::PI * freq * t).sin();
            let harm = 0.3 * (2.0 * 2.0 * std::f32::consts::PI * freq * t).sin();
            let amp = if kind == Sfx::Boop { 0.5 } else { 0.42 };
            let s = (wave + harm) * env * amp;
            samples.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
        pack_wav(&samples)
    }

    fn pack_wav(samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::with_capacity(44 + data_len as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&SR.to_le_bytes());
        b.extend_from_slice(&(SR * 2).to_le_bytes()); // byte rate
        b.extend_from_slice(&2u16.to_le_bytes()); // block align
        b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for &s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    pub(crate) fn cache(kind: Sfx) -> &'static [u8] {
        static POP: OnceLock<Vec<u8>> = OnceLock::new();
        static BOOP: OnceLock<Vec<u8>> = OnceLock::new();
        let slot = match kind { Sfx::Pop => &POP, Sfx::Boop => &BOOP };
        slot.get_or_init(|| make_wav(kind)).as_slice()
    }

    pub(crate) fn play(kind: Sfx) {
        let wav = cache(kind);
        unsafe {
            let _ = PlaySoundW(
                PCWSTR(wav.as_ptr() as *const u16),
                HMODULE::default(),
                SND_MEMORY | SND_ASYNC,
            );
        }
    }
}

/// 播放一段音效。非 Windows 平台为空实现。
pub fn play(kind: Sfx) {
    #[cfg(windows)]
    imp::play(kind);
    #[cfg(not(windows))]
    let _ = kind;
}

/// 缓存预热：预先合成两份 WAV 字节，首次真正播放更跟手。
/// 只在后台造数据，不发出声音。
pub fn warmup() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        #[cfg(windows)]
        {
            let _ = imp::cache(Sfx::Pop);
            let _ = imp::cache(Sfx::Boop);
        }
        #[cfg(not(windows))]
        {}
    });
}

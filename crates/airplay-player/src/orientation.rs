//! Landscape **game** vs portrait **home UI** detection.
//!
//! Two signals (no OCR / no app name from AirPlay):
//! 1. **Stream size** from SPS/type-1 headers: `width ≥ height` → game/landscape.
//! 2. **Letterbox** in a portrait frame: black bars top+bottom with a wide content
//!    band (Hoyoverse / Genshin style splash in a tall mirror) → game + crop.

/// Presentation mode for the live window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentMode {
    /// Phone home / portrait apps (default).
    #[default]
    HomePortrait,
    /// Landscape game or landscape app.
    GameLandscape,
}

impl ContentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HomePortrait => "home/portrait",
            Self::GameLandscape => "game/landscape",
        }
    }
}

/// How to classify from stream pixel size alone.
pub fn mode_from_stream_size(width: u32, height: u32) -> ContentMode {
    if width == 0 || height == 0 {
        return ContentMode::HomePortrait;
    }
    if width >= height {
        ContentMode::GameLandscape
    } else {
        ContentMode::HomePortrait
    }
}

/// Crop suggested after letterbox detection (pixels to remove top / bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LetterboxCrop {
    pub top: u32,
    pub bottom: u32,
}

impl LetterboxCrop {
    pub fn is_active(self) -> bool {
        self.top > 0 || self.bottom > 0
    }

    pub fn content_height(self, frame_height: u32) -> u32 {
        frame_height.saturating_sub(self.top.saturating_add(self.bottom))
    }
}

/// Result of scanning one decoded frame for letterboxed landscape content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LetterboxDetect {
    pub mode: ContentMode,
    pub crop: LetterboxCrop,
}

/// Max channel value treated as "black bar" (BGRx/RGBx 8-bit).
pub const BLACK_LUMA_THRESHOLD: u32 = 28;
/// Min fraction of frame height that must be black bars (top+bottom).
pub const MIN_BAR_FRACTION: f32 = 0.12;
/// Content band must be wider than tall by this ratio to count as landscape game.
pub const MIN_LANDSCAPE_ASPECT: f32 = 1.15;
/// Ignore tiny noise strips at the very edges (fraction of height).
pub const EDGE_IGNORE_FRACTION: f32 = 0.02;

/// Detect letterboxed landscape game content in a **portrait** frame.
///
/// `pixels`: tightly packed or strided 8-bit RGB/BGR/BGRx/RGBx (3 or 4 bytes/pixel).
/// Row `y` starts at `pixels[y * stride]`.
///
/// Returns `None` if the frame is not a clear letterboxed landscape game.
pub fn detect_letterbox_game(
    pixels: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    bytes_per_pixel: usize,
) -> Option<LetterboxDetect> {
    if width < 16 || height < 32 || bytes_per_pixel < 3 {
        return None;
    }
    // Only meaningful on tall frames (home UI advertise / portrait stream).
    if width >= height {
        return None;
    }
    let w = width as usize;
    let h = height as usize;
    let need = stride
        .checked_mul(h.saturating_sub(1))
        .and_then(|o| o.checked_add(w.checked_mul(bytes_per_pixel)?))?;
    if pixels.len() < need {
        return None;
    }

    let row_is_black = |y: usize| -> bool {
        let row = &pixels[y * stride..y * stride + w * bytes_per_pixel];
        // Sample every 4th pixel for speed.
        let mut sum: u64 = 0;
        let mut n: u64 = 0;
        let mut x = 0usize;
        while x < w {
            let i = x * bytes_per_pixel;
            let r = row[i] as u64;
            let g = row[i + 1] as u64;
            let b = row[i + 2] as u64;
            // Use max channel as cheap luma proxy (works for RGB and BGR order).
            sum += r.max(g).max(b);
            n += 1;
            x += 4;
        }
        if n == 0 {
            return true;
        }
        (sum / n) as u32 <= BLACK_LUMA_THRESHOLD
    };

    let edge = ((h as f32) * EDGE_IGNORE_FRACTION).round() as usize;
    let edge = edge.max(1).min(h / 8);

    let mut top = edge;
    while top < h.saturating_sub(edge) && row_is_black(top) {
        top += 1;
    }
    let mut bottom_idx = h.saturating_sub(1 + edge);
    while bottom_idx > top && row_is_black(bottom_idx) {
        bottom_idx = bottom_idx.saturating_sub(1);
    }

    // Rows [top ..= bottom_idx] are content; black above top and below bottom_idx.
    let top_bar = top as u32;
    let bottom_bar = (h.saturating_sub(1).saturating_sub(bottom_idx)) as u32;
    let content_h = bottom_idx.saturating_sub(top).saturating_add(1) as u32;
    if content_h < 8 {
        return None;
    }

    let bar_frac = (top_bar + bottom_bar) as f32 / height as f32;
    if bar_frac < MIN_BAR_FRACTION {
        return None;
    }

    let aspect = width as f32 / content_h as f32;
    if aspect < MIN_LANDSCAPE_ASPECT {
        return None;
    }

    // Require both top and bottom bars (Hoyoverse-style letterbox), not only one side.
    if top_bar < 4 || bottom_bar < 4 {
        return None;
    }

    Some(LetterboxDetect {
        mode: ContentMode::GameLandscape,
        crop: LetterboxCrop {
            top: top_bar,
            bottom: bottom_bar,
        },
    })
}

/// Hysteresis helper: require several consistent samples before switching mode.
#[derive(Debug, Default)]
pub struct ModeTracker {
    pub mode: ContentMode,
    pub crop: LetterboxCrop,
    game_votes: u8,
    home_votes: u8,
    /// Consecutive letterbox detections before applying crop.
    pub confirm_game: u8,
    /// Consecutive non-letterbox samples before clearing crop (when stream still portrait).
    pub confirm_home: u8,
}

impl ModeTracker {
    pub fn new() -> Self {
        Self {
            mode: ContentMode::HomePortrait,
            crop: LetterboxCrop::default(),
            game_votes: 0,
            home_votes: 0,
            confirm_game: 3,
            confirm_home: 8,
        }
    }

    /// Apply stream-size signal (strong): immediate switch, clear letterbox crop.
    pub fn on_stream_size(&mut self, width: u32, height: u32) -> bool {
        let next = mode_from_stream_size(width, height);
        self.game_votes = 0;
        self.home_votes = 0;
        if next == ContentMode::GameLandscape {
            // Native landscape stream — no letterbox crop needed.
            let changed = self.mode != next || self.crop.is_active();
            self.mode = next;
            self.crop = LetterboxCrop::default();
            changed
        } else {
            let changed = self.mode != next;
            self.mode = next;
            // Keep letterbox crop until samples clear it (game may still be letterboxed).
            changed
        }
    }

    /// Apply one letterbox sample. Returns true if mode or crop changed.
    pub fn on_letterbox_sample(&mut self, sample: Option<LetterboxDetect>) -> bool {
        // Only refine portrait streams (native landscape already GameLandscape).
        if self.mode == ContentMode::GameLandscape && !self.crop.is_active() {
            // Stream size already said landscape; ignore letterbox.
            return false;
        }

        match sample {
            Some(det) if det.mode == ContentMode::GameLandscape => {
                self.home_votes = 0;
                self.game_votes = self.game_votes.saturating_add(1);
                if self.game_votes >= self.confirm_game {
                    let changed = self.mode != ContentMode::GameLandscape || self.crop != det.crop;
                    self.mode = ContentMode::GameLandscape;
                    self.crop = det.crop;
                    return changed;
                }
            }
            _ => {
                self.game_votes = 0;
                self.home_votes = self.home_votes.saturating_add(1);
                if self.home_votes >= self.confirm_home {
                    let changed = self.mode != ContentMode::HomePortrait || self.crop.is_active();
                    self.mode = ContentMode::HomePortrait;
                    self.crop = LetterboxCrop::default();
                    return changed;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame(w: u32, h: u32, content_top: u32, content_bottom: u32) -> (Vec<u8>, usize) {
        // BGRx 4 bytes/pixel
        let bpp = 4usize;
        let stride = w as usize * bpp;
        let mut px = vec![0u8; stride * h as usize]; // black
        for y in content_top..content_bottom.min(h) {
            for x in 0..w as usize {
                let i = y as usize * stride + x * bpp;
                px[i] = 40; // B
                px[i + 1] = 80; // G
                px[i + 2] = 200; // R — bright content
                px[i + 3] = 255;
            }
        }
        (px, stride)
    }

    #[test]
    fn stream_size_home_vs_game() {
        assert_eq!(mode_from_stream_size(1080, 1920), ContentMode::HomePortrait);
        assert_eq!(
            mode_from_stream_size(1920, 1080),
            ContentMode::GameLandscape
        );
        assert_eq!(mode_from_stream_size(1280, 720), ContentMode::GameLandscape);
        assert_eq!(mode_from_stream_size(720, 1280), ContentMode::HomePortrait);
    }

    #[test]
    fn letterbox_hoyoverse_style() {
        // Portrait 1080x1920 with ~16:9 band in the middle (~607px content).
        let w = 1080u32;
        let h = 1920u32;
        let content_h = 608u32;
        let top = (h - content_h) / 2;
        let bottom = top + content_h;
        let (px, stride) = make_frame(w, h, top, bottom);
        let det = detect_letterbox_game(&px, w, h, stride, 4).expect("letterbox game");
        assert_eq!(det.mode, ContentMode::GameLandscape);
        assert!(det.crop.top > 100);
        assert!(det.crop.bottom > 100);
        assert!(det.crop.content_height(h) < h);
        let aspect = w as f32 / det.crop.content_height(h) as f32;
        assert!(aspect >= MIN_LANDSCAPE_ASPECT);
    }

    #[test]
    fn full_frame_content_not_letterbox() {
        let w = 1080u32;
        let h = 1920u32;
        let (px, stride) = make_frame(w, h, 0, h);
        assert!(detect_letterbox_game(&px, w, h, stride, 4).is_none());
    }

    #[test]
    fn landscape_frame_skipped() {
        let w = 1920u32;
        let h = 1080u32;
        let (px, stride) = make_frame(w, h, 100, 980);
        assert!(detect_letterbox_game(&px, w, h, stride, 4).is_none());
    }

    #[test]
    fn tracker_hysteresis() {
        let mut t = ModeTracker::new();
        t.confirm_game = 2;
        t.confirm_home = 2;
        assert_eq!(t.mode, ContentMode::HomePortrait);

        let crop = LetterboxCrop {
            top: 400,
            bottom: 400,
        };
        let det = LetterboxDetect {
            mode: ContentMode::GameLandscape,
            crop,
        };
        assert!(!t.on_letterbox_sample(Some(det)));
        assert!(t.on_letterbox_sample(Some(det)));
        assert_eq!(t.mode, ContentMode::GameLandscape);
        assert_eq!(t.crop, crop);

        assert!(!t.on_letterbox_sample(None));
        assert!(t.on_letterbox_sample(None));
        assert_eq!(t.mode, ContentMode::HomePortrait);
        assert!(!t.crop.is_active());
    }

    #[test]
    fn tracker_native_landscape_stream() {
        let mut t = ModeTracker::new();
        assert!(t.on_stream_size(1920, 1080));
        assert_eq!(t.mode, ContentMode::GameLandscape);
        assert!(!t.crop.is_active());
        assert!(t.on_stream_size(1080, 1920));
        assert_eq!(t.mode, ContentMode::HomePortrait);
    }
}

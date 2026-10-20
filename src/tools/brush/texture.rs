//! 笔尖纹理：把一张 PNG 当成**覆盖率蒙版**（白 = 完全落笔、黑 = 不落笔）。
//!
//! 纹理只提供「笔尖哪里厚哪里薄」，颜色仍然由前景色决定，所以一套素材能配任意
//! 颜色，也不需要给每种颜色存一张图。
//!
//! 取值规则：PNG 有 alpha 通道就用 alpha（我们生成的素材就是 RGB 全白 + alpha），
//! 没有 alpha 或整张不透明（编辑器里另外画的蒙版）就退回**亮度**。
//!
//! 采样在笔尖本地坐标里做（`0..1`，圆心在 `0.5`）：绘图用双线性（抗锯齿），
//! 像素模式用最近邻（硬边）。越界一律 0 —— 笔尖之外不落笔。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::document::PixelBuffer;

/// 一张笔尖纹理（只有覆盖率，8 位）。
#[derive(Clone, PartialEq)]
pub struct BrushTexture {
    width: u32,
    height: u32,
    /// 行优先覆盖率，`0..=255`。
    coverage: Vec<u8>,
    /// 平均覆盖率（0..=1）：颗粒的对比度以它为轴，才能把"整体偏白"的素材拉开。
    mean: f32,
}

impl BrushTexture {
    /// 从一张 RGBA 图像取覆盖率（alpha；整张不透明时取亮度）。
    pub fn from_pixels(pixels: &PixelBuffer) -> Option<Self> {
        if pixels.width == 0 || pixels.height == 0 {
            return None;
        }
        let mut coverage = Vec::with_capacity((pixels.width * pixels.height) as usize);
        let opaque =
            (0..pixels.height).all(|y| (0..pixels.width).all(|x| pixels.get_pixel(x, y).a == 255));
        for y in 0..pixels.height {
            for x in 0..pixels.width {
                let color = pixels.get_pixel(x, y);
                coverage.push(if opaque {
                    // 没有 alpha 的图：白色 = 密、黑色 = 疏。
                    ((color.r as u32 * 30 + color.g as u32 * 59 + color.b as u32 * 11) / 100) as u8
                } else {
                    color.a
                });
            }
        }
        let mean = coverage
            .iter()
            .map(|value| f32::from(*value) / 255.0)
            .sum::<f32>()
            / coverage.len() as f32;
        Some(Self {
            width: pixels.width,
            height: pixels.height,
            coverage,
            mean,
        })
    }

    /// 从 PNG 字节建一张笔尖纹理。
    pub fn from_png(bytes: &[u8]) -> Option<Self> {
        let pixels = crate::io::decode_png(bytes).ok()?;
        Self::from_pixels(&pixels)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 平均覆盖率（0..=1）。颗粒的对比度 / 亮度都以它为中性点。
    pub fn mean(&self) -> f32 {
        self.mean
    }

    /// 采样：`u` / `v` 是笔尖本地坐标（`0..1`，越界返回 0）。
    ///
    /// `linear = false` 用最近的像素中心（像素模式），`true` 用双线性（绘图）。
    pub fn sample(&self, u: f32, v: f32, linear: bool) -> f32 {
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            return 0.0;
        }
        let (width, height) = (self.width as f32, self.height as f32);
        if !linear {
            let x = (u * width) as u32;
            let y = (v * height) as u32;
            return self.at(x.min(self.width - 1), y.min(self.height - 1)) as f32 / 255.0;
        }
        // 双线性：像素中心在 `(i + 0.5) / width`。
        let px = u * width - 0.5;
        let py = v * height - 0.5;
        let x0 = px.floor();
        let y0 = py.floor();
        let fx = px - x0;
        let fy = py - y0;
        let lerp = |a: u32, b: u32, t: f32| -> f32 {
            self.at(a.min(self.width - 1), b.min(self.height - 1)) as f32 * t
        };
        let sample = |xi: i64, yi: i64| -> f32 {
            let (x, y) = (
                xi.clamp(0, self.width as i64 - 1),
                yi.clamp(0, self.height as i64 - 1),
            );
            self.at(x as u32, y as u32) as f32 / 255.0
        };
        let _ = lerp;
        let top = sample(x0 as i64, y0 as i64) * (1.0 - fx) + sample(x0 as i64 + 1, y0 as i64) * fx;
        let bottom = sample(x0 as i64, y0 as i64 + 1) * (1.0 - fx)
            + sample(x0 as i64 + 1, y0 as i64 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    fn at(&self, x: u32, y: u32) -> u8 {
        self.coverage[(y * self.width + x) as usize]
    }
}

impl std::fmt::Debug for BrushTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrushTexture")
            .field("size", &(self.width, self.height))
            .finish()
    }
}

/// 素材目录（和随包字体一样：crate 根 → 可执行文件旁 → 当前目录）。
const ASSET_DIR: &str = "assets/brushes";
/// 想让编辑器用自己那套 PNG 时，指向一个目录即可（同 `IMAGE_EDITOR_ICON_DIR`）。
const ENV_DIR: &str = "IMAGE_EDITOR_BRUSH_DIR";

/// 运行时素材库：按名字懒加载并缓存（`Rc` 共享，克隆便宜）。
pub struct BrushTextures {
    dir: Option<PathBuf>,
    cache: RefCell<HashMap<String, Option<Rc<BrushTexture>>>>,
}

impl Default for BrushTextures {
    fn default() -> Self {
        Self::new(find_asset_dir())
    }
}

impl BrushTextures {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// 素材目录（用户预设 `presets.txt` 也从这里读）。
    pub fn dir(&self) -> Option<&PathBuf> {
        self.dir.as_ref()
    }

    /// 反查一张纹理的名字（写配置用）；缓存里找不到就 `None`。
    pub fn name_of(&self, wanted: &Rc<BrushTexture>) -> Option<String> {
        self.cache
            .borrow()
            .iter()
            .find(|(_, cached)| {
                cached
                    .as_ref()
                    .is_some_and(|texture| Rc::ptr_eq(texture, wanted))
            })
            .map(|(name, _)| name.clone())
    }

    /// 按名字取一张纹理；文件缺失 / 读失败时返回 `None`（调用方退回程序化笔尖）。
    pub fn get(&self, name: &str) -> Option<Rc<BrushTexture>> {
        if let Some(cached) = self.cache.borrow().get(name) {
            return cached.clone();
        }
        let loaded = self.dir.as_ref().and_then(|dir| {
            let bytes = std::fs::read(dir.join(format!("{name}.png"))).ok()?;
            BrushTexture::from_png(&bytes).map(Rc::new)
        });
        self.cache
            .borrow_mut()
            .insert(name.to_string(), loaded.clone());
        loaded
    }

    /// 素材目录里有哪些笔尖（预设列表 / 报告用）。
    pub fn names(&self) -> Vec<String> {
        let Some(dir) = self.dir.as_ref() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension()? == "png")
                    .then(|| path.file_stem()?.to_str().map(str::to_string))
                    .flatten()
            })
            .collect();
        names.sort();
        names
    }
}

fn find_asset_dir() -> Option<PathBuf> {
    // 用户自己的素材目录优先（和图标包一个套路）。
    if let Ok(dir) = std::env::var(ENV_DIR) {
        let dir = PathBuf::from(dir);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(root) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(root).join(ASSET_DIR));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(ASSET_DIR));
        }
    }
    candidates.push(PathBuf::from(ASSET_DIR));
    candidates.into_iter().find(|path| path.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Color;

    /// 2×2 纹理：左上满覆盖、右上 50%、左下 25%、右下 0。
    fn tiny() -> BrushTexture {
        let mut pixels = PixelBuffer::new(2, 2);
        pixels.set_pixel(0, 0, Color::new(255, 255, 255, 255));
        pixels.set_pixel(1, 0, Color::new(255, 255, 255, 128));
        pixels.set_pixel(0, 1, Color::new(255, 255, 255, 64));
        pixels.set_pixel(1, 1, Color::new(255, 255, 255, 0));
        BrushTexture::from_pixels(&pixels).expect("2×2")
    }

    #[test]
    fn a_texture_takes_its_alpha_and_falls_back_to_luminance() {
        let texture = tiny();
        assert_eq!(texture.sample(0.25, 0.25, false), 1.0);
        assert!((texture.sample(0.75, 0.25, false) - 128.0 / 255.0).abs() < 1e-6);

        // 整张不透明：改用亮度（白 = 密、黑 = 疏）。
        let mut pixels = PixelBuffer::new(2, 1);
        pixels.set_pixel(0, 0, Color::WHITE);
        pixels.set_pixel(1, 0, Color::BLACK);
        let lum = BrushTexture::from_pixels(&pixels).expect("2×1");
        assert_eq!(lum.sample(0.25, 0.5, false), 1.0);
        assert_eq!(lum.sample(0.75, 0.5, false), 0.0);
    }

    #[test]
    fn sampling_outside_the_tip_is_zero() {
        let texture = tiny();
        assert_eq!(texture.sample(-0.1, 0.5, false), 0.0);
        assert_eq!(texture.sample(0.5, 1.0, false), 0.0);
        assert_eq!(
            texture.sample(0.5, 0.5, true),
            texture.sample(0.5, 0.5, true)
        );
    }

    #[test]
    fn bilinear_sampling_blends_between_pixels() {
        // 1×2：上 0、下 255 —— 垂直中点应该是 ~0.5。
        let mut pixels = PixelBuffer::new(1, 2);
        pixels.set_pixel(0, 0, Color::new(255, 255, 255, 0));
        pixels.set_pixel(0, 1, Color::new(255, 255, 255, 255));
        let texture = BrushTexture::from_pixels(&pixels).expect("1×2");
        let middle = texture.sample(0.5, 0.5, true);
        assert!((middle - 0.5).abs() < 1e-3, "middle = {middle}");
        // 最近邻：取像素中心的那个（下半）。
        assert_eq!(texture.sample(0.5, 0.5, false), 1.0);
    }

    #[test]
    fn a_png_round_trips_through_the_texture() {
        // 用我们自己的编码器造一张 PNG，再从字节加载回来。
        let mut pixels = PixelBuffer::new(4, 4);
        pixels.set_pixel(1, 1, Color::new(255, 255, 255, 200));
        let bytes = crate::io::encode_png(&pixels).expect("编码");
        let texture = BrushTexture::from_png(&bytes).expect("从 PNG 加载");
        assert_eq!(texture.size(), (4, 4));
        assert!((texture.sample((1.5) / 4.0, (1.5) / 4.0, false) - 200.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn the_real_assets_load_and_are_not_empty() {
        // 仓库自带的素材：能加载、确实有覆盖（不是全黑）。
        let textures = BrushTextures::default();
        let names = textures.names();
        assert!(
            names.iter().any(|name| name == "ink"),
            "assets/brushes 里应有 ink.png：{names:?}"
        );
        let ink = textures.get("ink").expect("加载 ink.png");
        let solid = (0..ink.height)
            .flat_map(|y| (0..ink.width).map(move |x| (x, y)))
            .filter(|(x, y)| ink.at(*x, *y) > 128)
            .count();
        assert!(solid > 1000, "ink 应该有实心区域，实际 {solid} 像素");
        assert!(
            textures.get("没有这张").is_none(),
            "缺素材返回 None 而不是 panic"
        );
    }
}

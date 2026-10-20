//! 生成 `assets/brushes/` 里的笔尖素材（覆盖率蒙版 PNG）。
//!
//! 跑一次、把产物提交进 git：
//!
//! ```bash
//! cargo run --example gen_brushes
//! ```
//!
//! 为什么要脚本生成：这些是**蒙版**（白 = 完全落笔、黑 = 不落笔），只表达「笔尖
//! 哪里厚哪里薄」，颜色由前景色决定。所以它不需要美术资源，用确定性的伪随机
//! 画出毛束 / 湿边就够了，而且仓库里放得下（每张几 KB）。

use std::path::PathBuf;

/// 素材边长（笔尖纹理是正方形，按直径缩放）。
const SIZE: u32 = 128;

/// 确定性伪随机（xorshift32）：同一个脚本每次生成同样的图。
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// 0..1
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1 << 24) as f32
    }
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/brushes");
    std::fs::create_dir_all(&dir).expect("创建 assets/brushes");

    for (name, mask) in [
        ("ink", ink_mask(SIZE)),
        ("bristle", bristle_mask(SIZE)),
        ("grass", grass_mask(SIZE)),
    ] {
        let path = dir.join(format!("{name}.png"));
        write_mask(&path, SIZE, &mask);
        println!("写好了 {}", path.display());
    }
}

/// 毛笔：圆润的湿边主体 + 一圈更深的「水痕」+ 沿笔势的细毛。
fn ink_mask(size: u32) -> Vec<u8> {
    let mut rng = Rng(0x1234_5678);
    let mut mask = vec![0u8; (size * size) as usize];
    let center = size as f32 / 2.0;
    let radius = center * 0.96;
    // 沿纹理 x 轴的细毛（随机的粗细 / 深浅），让边缘不是完美圆。
    let mut fringe = [0.0f32; 256];
    let mut x = 0;
    while x < fringe.len() {
        fringe[x] = 0.86 + rng.unit() * 0.14;
        x += 1;
    }
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt() / radius;
            if distance >= 1.0 {
                continue;
            }
            // 主体：中心实、到 0.82 半径开始淡出（湿边）。
            let body = ((1.0 - distance) / 0.18).clamp(0.0, 1.0).sqrt();
            // 水痕：0.78~0.95 半径之间更深一点（毛笔外沿积墨）。
            let rim = if (0.74..0.97).contains(&distance) {
                0.55 * ((distance - 0.74) / 0.23).sin().max(0.0)
            } else {
                0.0
            };
            let hair = fringe[(x as usize) % fringe.len()];
            let value = ((body * 0.92 + rim) * hair).clamp(0.0, 1.0);
            mask[(y * size + x) as usize] = (value * 255.0) as u8;
        }
    }
    mask
}

/// 刷子：一束竖直的硬毛，长短 / 深浅不一，两端收圆。
fn bristle_mask(size: u32) -> Vec<u8> {
    let mut rng = Rng(0x9e37_79b9);
    let mut mask = vec![0u8; (size * size) as usize];
    let center = size as f32 / 2.0;
    let radius = center * 0.98;
    // 一束毛：每条有中心 x、宽度、强度、长度。
    struct Hair {
        x: f32,
        half: f32,
        alpha: f32,
        reach: f32,
    }
    let mut hairs = Vec::new();
    let count = 40;
    for index in 0..count {
        let t = (index as f32 + 0.5) / count as f32;
        let jitter = (rng.unit() - 0.5) * 6.0;
        hairs.push(Hair {
            x: -radius + t * radius * 2.0 + jitter,
            half: 1.8 + rng.unit() * 1.6,
            alpha: 0.7 + rng.unit() * 0.3,
            reach: radius * (0.55 + rng.unit() * 0.45),
        });
    }
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            // 底色：圆内给一层薄薄的湿痕，毛束之间的缝就不会看着像「散点」。
            let body = ((radius - dy.abs().max(dx.abs())) / (radius * 0.6)).clamp(0.0, 1.0);
            let mut value = body * 0.28;
            for hair in &hairs {
                let lateral = (dx - hair.x).abs();
                if lateral > hair.half {
                    continue;
                }
                // 竖直方向：|dy| 超过这条毛的 reach 就断掉，边缘 1px 收圆。
                let along = (hair.reach - dy.abs()) / 1.5;
                if along <= 0.0 {
                    continue;
                }
                let across = (1.0 - lateral / hair.half).sqrt();
                value = value.max(hair.alpha * across.min(1.0) * along.min(1.0));
            }
            mask[(y * size + x) as usize] = (value.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    mask
}

/// 草：一小簇细长的叶子，从下方中心散开（散布笔用，每个 dab 随机旋转）。
fn grass_mask(size: u32) -> Vec<u8> {
    let mut rng = Rng(0xdead_beef);
    let mut mask = vec![0u8; (size * size) as usize];
    let root = (size as f32 / 2.0, size as f32 * 0.92);
    let blades = 9;
    for _ in 0..blades {
        let lean = (rng.unit() - 0.5) * 1.6; // 横向倾斜
        let length = size as f32 * (0.45 + rng.unit() * 0.5);
        let width = 1.2 + rng.unit() * 1.8;
        let strength = 0.6 + rng.unit() * 0.4;
        let steps = (length * 2.0) as usize;
        for step in 0..steps {
            let t = step as f32 / steps.max(1) as f32;
            // 二次曲线：根部竖直向上、叶尖向外倒。
            let bend = lean * t * t;
            let px = root.0 + bend * size as f32 * 0.5;
            let py = root.1 - t * length;
            let taper = (1.0 - t).sqrt() * width;
            let x0 = (px - taper).floor() as i64;
            let x1 = (px + taper).ceil() as i64;
            let y = py.floor() as i64;
            for x in x0..=x1 {
                if x < 0 || y < 0 || x >= size as i64 || y >= size as i64 {
                    continue;
                }
                let falloff = (1.0 - ((x as f32 + 0.5 - px) / taper.max(0.5)).abs()).max(0.0);
                let value = strength * falloff * (1.0 - t * 0.35);
                let index = (y as u32 * size + x as u32) as usize;
                mask[index] = mask[index].max((value.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    mask
}

/// 把 8 位覆盖率写成带 alpha 的 RGBA PNG（RGB 全白，颜色由前景色决定）。
fn write_mask(path: &std::path::Path, size: u32, mask: &[u8]) {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for value in mask {
        rgba.extend_from_slice(&[255, 255, 255, *value]);
    }
    let file = std::fs::File::create(path).expect("创建 PNG");
    let mut encoder = png::Encoder::new(file, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("写 PNG 头");
    writer.write_image_data(&rgba).expect("写 PNG 数据");
    writer.finish().expect("收尾 PNG");
}

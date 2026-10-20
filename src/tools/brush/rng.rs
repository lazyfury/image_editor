//! 一笔里的随机数：XorShift128，种子**每笔一次**。
//!
//! 为什么要自己写一个：`rand` 不在依赖里；而且**可复现**是硬要求 —— 同一个种子 +
//! 同一串采样点必须压出完全相同的像素，散布 / 抖动的测试才断得出来。
//!
//! 用法：`begin_stroke` 里 `Rng::new(seed)` 一次（种子来自笔刷自己的计数器，所以
//! 连画两笔也不一样），之后每个 dab 从它身上取偏移 / 角度 / 抖动。

use igui_core::Vec2;

/// splitmix64：只用来把 64 位种子摊开成 4 个 32 位状态，避免小种子前几个数太小。
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// XorShift128 随机数发生器（状态 4×u32，永不为全 0）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    state: [u32; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut mixer = seed;
        let mut state = [0u32; 4];
        for slot in state.iter_mut() {
            let value = splitmix64(&mut mixer);
            *slot = (value >> 32) as u32 ^ (value as u32);
        }
        if state == [0; 4] {
            // XorShift 全 0 会永远输出 0。
            state = [0x9E37_79B9, 0x243F_6A88, 0xB7E1_5162, 0x85EB_CA6B];
        }
        Self { state }
    }

    pub fn next_u32(&mut self) -> u32 {
        let [x, y, z, w] = self.state;
        let t = x ^ (x << 11);
        let next = w ^ (w >> 19) ^ (t ^ (t >> 8));
        self.state = [y, z, w, next];
        next
    }

    /// 0..1（取高 24 位，浮点步长均匀）。
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
    }

    /// 半径 1 的圆盘上**均匀**取点（`sqrt` 才不会往圆心堆）。
    pub fn unit_disc(&mut self) -> Vec2 {
        let angle = self.unit() * std::f32::consts::TAU;
        let radius = self.unit().sqrt();
        Vec2::new(angle.cos() * radius, angle.sin() * radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_replays_the_same_numbers() {
        // 散布 / 抖动要靠这个：同种子 -> 同一串数（测试才能断言像素）。
        let a: Vec<u32> = (0..8).map(|_| Rng::new(7).next_u32()).collect();
        let mut rng = Rng::new(7);
        let b: Vec<u32> = (0..8).map(|_| rng.next_u32()).collect();
        assert_eq!(a[0], b[0], "同种子第一个数应该一样");
        let mut first = Rng::new(7);
        let mut second = Rng::new(7);
        let sequence: Vec<f32> = (0..64).map(|_| first.unit()).collect();
        let again: Vec<f32> = (0..64).map(|_| second.unit()).collect();
        assert_eq!(sequence, again);
        assert_ne!(
            sequence,
            (0..64).map(|_| Rng::new(8).unit()).collect::<Vec<_>>(),
            "换种子要换序列"
        );
    }

    #[test]
    fn unit_stays_in_range_and_spreads_over_the_interval() {
        let mut rng = Rng::new(1);
        let mut buckets = [0usize; 10];
        for _ in 0..10_000 {
            let value = rng.unit();
            assert!((0.0..1.0).contains(&value), "unit 越界：{value}");
            buckets[(value * 10.0) as usize] += 1;
        }
        // 一万个数分十格：每格都该有，且没有一格吃掉一半。
        assert!(buckets.iter().all(|count| *count > 500), "{buckets:?}");
        assert!(buckets.iter().all(|count| *count < 1500), "{buckets:?}");
    }

    #[test]
    fn a_unit_disc_fills_all_four_quadrants_and_never_leaks() {
        // 散射点不能出圆（否则笔迹会比笔刷大），也不能只往一角偏。
        let mut rng = Rng::new(3);
        let mut quadrants = [0usize; 4];
        let mut max_radius: f32 = 0.0;
        for _ in 0..10_000 {
            let point = rng.unit_disc();
            let radius = point.length();
            assert!(radius <= 1.000_001, "出圆了：{radius}");
            max_radius = max_radius.max(radius);
            let index = (point.x >= 0.0) as usize + ((point.y >= 0.0) as usize) * 2;
            quadrants[index] += 1;
        }
        assert!(max_radius > 0.99, "要能到边缘：{max_radius}");
        assert!(quadrants.iter().all(|count| *count > 2000), "{quadrants:?}");
    }
}

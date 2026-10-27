//! igui 图像编辑器 —— 用 igui 的 UI 栈代替 `egui` 的桌面示例。
//!
//! 启动先看到**主页**（[`ui::HomeView`]）；点「新建项目」在主窗口里打开
//! 一扇**模拟桌面的浮动窗口**（[`ui::NewDocumentModal`]：标题栏可拖动移动、
//! 四条边 / 角可拖动缩放）选尺寸 / 背景色，「画廊」是占位；创建后编辑器仍在
//! **同一个窗口**里。单窗口 + `igui_app` 运行时由 [`app::application`] 组装。
//!
//! 编辑器本身到 Phase 8（移动 / 框选 / 吸管）：菜单/工具栏 + 真实文档 /
//! 图层 + 画笔 / 橡皮 + 撤销 / 重做 + PNG 导入 / 导出 + 完整工具集。
//!
//! ```bash
//! cargo run --manifest-path examples/image_editor/Cargo.toml
//! cargo run --manifest-path examples/image_editor/Cargo.toml -- --selfcheck
//! ```

mod app;
#[cfg(test)]
mod architecture;
mod bench;
mod canvas;
mod document;
mod fonts;
mod icons;
mod io;
mod renderer;
mod selfcheck;
mod theme;
mod tools;
mod ui;

use app::application::{self, Options};

const HELP: &str = "\
image_editor — 用 igui 画的图像编辑器（Phase 8：移动 / 框选 / 吸管）

启动先显示主页；点「新建项目」在主窗口里弹出一扇**模拟桌面的浮动窗口**
（标题栏拖动移动、四条边 / 角拖动缩放、右上关闭、Esc / 点窗口外取消）
选尺寸 / 背景色，创建后在同一个窗口里编辑。「画廊」是占位。

用法:
  image_editor [选项]

选项:
      --selfcheck   无头自检：录 DrawList + igui_profile 体检 + 断言关键内容，
                    失败退出码 1
      --dump        同 --selfcheck，并打印完整绘制命令
      --bench       图层 / 图层树 UI 的性能基准（建议配 --release 跑）
      --frames <n>  渲染 n 帧后退出（验证真实渲染管线）
      --light       使用浅色主题（默认深色）
      --pixel-font  使用内置点阵字体（默认系统字体；点阵字体不含中文）
      --ui-menu     菜单栏画在窗口里（默认用 macOS 系统菜单栏）
  -h, --help        显示本帮助

快捷键:
  V 移动 · B 画笔 · E 橡皮 · M 矩形选择 · L 套索 · G 油漆桶 · W 魔棒 · N 渐变 · R 矩形填充 · U 自由填充 · I 吸管
  Ctrl/Cmd+Z 撤销 · Shift+Ctrl/Cmd+Z（或 Ctrl+Y）重做
  Ctrl/Cmd+S 保存 · Shift+Ctrl/Cmd+S 另存为 · Ctrl/Cmd+O 打开 · Ctrl/Cmd+E 导出 PNG · Ctrl/Cmd+N 新建
  [ ] 笔刷大小 · , . 笔刷不透明度
  + - 缩放 · 0 100% · F 适配

文件（Phase 7 / 18–23）:
  「文件」菜单 / 快捷键：保存 / 另存为… / 打开… / 导入 PNG… / 导出 PNG…
  都走**原生文件对话框**（手写平台 FFI：macOS 用 NSOpenPanel / NSSavePanel）
  「保存」有路径就直接写、没有就等同「另存为」；关闭有未保存改动的工作区先确认
  顶部标签条切换 / 关闭打开的工作区；主页画廊列出打开中 / 最近工作区
  把 PNG 拖进窗口 = 作为新图层导入（没打开工作区时按图片尺寸新建文档）

工具（Phase 8 / 12–16）:
  工具栏把相关工具收成 7 个组按钮（移动 / 画笔 / 橡皮 / 选区 / 油漆桶 / 填充形状 / 吸管）：
  短按激活组内当前工具，长按（按住 ~350ms）向右展开组内全部工具
  V 移动当前图层（画布上拖动）
  B 画笔 · E 橡皮（各自单独一组）
  M 框选 · L 套索 · W 魔棒（同一组，长按切换）
  G 油漆桶 · N 渐变（同一组；渐变选项栏切类型 / 编辑多节点颜色）
  R 矩形填充 · U 自由填充（同一组，长按切换）
  I 吸管（点画布取色为前景色）
  选区存在时，画笔 / 填充 / 渐变都只在选区内生效
";

/// 解析后的命令行。
enum Command {
    Run(Options),
    /// 无头自检。`dump` 额外打印所有绘制命令。
    SelfCheck {
        dump: bool,
    },
    /// 图层 / 图层树 UI 性能基准。
    Bench,
    Help,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Command::Help) => print!("{HELP}"),
        Ok(Command::SelfCheck { dump }) => std::process::exit(selfcheck::run(dump)),
        Ok(Command::Bench) => std::process::exit(bench::run()),
        Ok(Command::Run(options)) => application::run(options),
        Err(message) => {
            eprintln!("error: {message}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}

fn parse(args: &[String]) -> Result<Command, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "--selfcheck" => return Ok(Command::SelfCheck { dump: false }),
            "--dump" => return Ok(Command::SelfCheck { dump: true }),
            "--bench" => return Ok(Command::Bench),
            "--light" => options.light = true,
            "--pixel-font" => options.pixel_font = true,
            "--ui-menu" => options.native_menu = false,
            "--frames" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| "--frames 需要一个帧数".to_string())?;
                index += 1;
                let frames: u32 = value
                    .parse()
                    .map_err(|_| format!("--frames 需要数字，收到 `{value}`"))?;
                if frames == 0 {
                    return Err("--frames 至少为 1".to_string());
                }
                options.frames = Some(frames);
            }
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("未知参数 `{other}`")),
        }
    }
    Ok(Command::Run(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn no_arguments_opens_a_window_with_defaults() {
        match parse(&args(&[])) {
            Ok(Command::Run(options)) => {
                assert_eq!(options.frames, None);
                assert!(!options.light);
                assert!(!options.pixel_font);
                // 默认用系统菜单栏（`todo.md` §1.aa）。
                assert!(options.native_menu);
            }
            _ => panic!("expected a window run"),
        }
    }

    #[test]
    fn selfcheck_and_dump_are_headless() {
        assert!(matches!(
            parse(&args(&["--selfcheck"])),
            Ok(Command::SelfCheck { dump: false })
        ));
        assert!(matches!(
            parse(&args(&["--dump"])),
            Ok(Command::SelfCheck { dump: true })
        ));
    }

    #[test]
    fn flags_are_collected() {
        match parse(&args(&["--frames", "3", "--light", "--pixel-font"])) {
            Ok(Command::Run(options)) => {
                assert_eq!(options.frames, Some(3));
                assert!(options.light);
                assert!(options.pixel_font);
            }
            _ => panic!("expected a window run"),
        }
    }

    /// `--ui-menu` 退回自绘菜单（系统菜单栏是默认）。
    #[test]
    fn the_ui_menus_opt_out_of_the_native_menu_bar() {
        match parse(&args(&["--ui-menu"])) {
            Ok(Command::Run(options)) => assert!(!options.native_menu),
            _ => panic!("expected a window run"),
        }
    }

    #[test]
    fn bad_arguments_are_reported() {
        assert!(parse(&args(&["--nope"])).is_err());
        assert!(parse(&args(&["--frames"])).is_err());
        assert!(parse(&args(&["--frames", "zero"])).is_err());
        assert!(parse(&args(&["--frames", "0"])).is_err());
    }

    #[test]
    fn help_wins() {
        assert!(matches!(parse(&args(&["--help"])), Ok(Command::Help)));
    }
}

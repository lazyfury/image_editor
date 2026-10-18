//! Windows 原生文件对话框：`comdlg32` 的 `GetOpenFileNameW` /
//! `GetSaveFileNameW`（**裸 FFI**，不引 `windows` crate）。
//!
//! 本仓库只在 macOS 上验证过，这份实现**未在本机验证**（`#[cfg]` 门控，不会
//! 影响 macOS 构建）。

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use super::{DialogOptions, FileDialog};

const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
const OFN_EXPLORER: u32 = 0x0008_0000;

/// `lpstrFile` 缓冲区的 `TCHAR` 数（够放长路径）。
const FILE_BUFFER_LEN: usize = 32_768;

/// `OPENFILENAMEW`（`commdlg.h`）。
#[repr(C)]
struct OpenFileNameW {
    l_struct_size: u32,
    hwnd_owner: *mut c_void,
    h_instance: *mut c_void,
    lpstr_filter: *const u16,
    lpstr_custom_filter: *mut u16,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u16,
    n_max_file: u32,
    lpstr_file_title: *mut u16,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u16,
    lpstr_title: *const u16,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u16,
    l_cust_data: isize,
    lpfn_hook: *mut c_void,
    lp_template_name: *const u16,
    pv_reserved: *mut c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
    fn GetSaveFileNameW(ofn: *mut OpenFileNameW) -> i32;
}

/// Windows 实现（无状态）。
#[derive(Default)]
pub struct NativeFileDialog;

impl FileDialog for NativeFileDialog {
    fn pick_open(&self, options: &DialogOptions) -> Option<PathBuf> {
        run(options, false)
    }

    fn pick_save(&self, options: &DialogOptions) -> Option<PathBuf> {
        run(options, true)
    }
}

fn run(options: &DialogOptions, save: bool) -> Option<PathBuf> {
    let filter = build_filter(options);
    let title = wide_nul(&options.title);
    let initial = options.start_dir.as_ref().map(|dir| wide_nul_path(dir));
    let default_extension = options
        .primary_extension()
        .map(wide_nul)
        .unwrap_or_else(|| vec![0]);

    let mut file_buffer = vec![0u16; FILE_BUFFER_LEN];
    if let Some(name) = options.default_name.as_deref() {
        let name: Vec<u16> = name.encode_utf16().collect();
        let len = name.len().min(file_buffer.len() - 1);
        file_buffer[..len].copy_from_slice(&name[..len]);
    }

    let mut flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST;
    if save {
        flags |= OFN_OVERWRITEPROMPT;
    }

    let mut ofn = OpenFileNameW {
        l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
        hwnd_owner: std::ptr::null_mut(),
        h_instance: std::ptr::null_mut(),
        lpstr_filter: filter.as_ptr(),
        lpstr_custom_filter: std::ptr::null_mut(),
        n_max_cust_filter: 0,
        n_filter_index: 1,
        lpstr_file: file_buffer.as_mut_ptr(),
        n_max_file: file_buffer.len() as u32,
        lpstr_file_title: std::ptr::null_mut(),
        n_max_file_title: 0,
        lpstr_initial_dir: initial
            .as_ref()
            .map_or(std::ptr::null(), |buffer| buffer.as_ptr()),
        lpstr_title: title.as_ptr(),
        flags,
        n_file_offset: 0,
        n_file_extension: 0,
        lpstr_def_ext: default_extension.as_ptr(),
        l_cust_data: 0,
        lpfn_hook: std::ptr::null_mut(),
        lp_template_name: std::ptr::null(),
        pv_reserved: std::ptr::null_mut(),
        dw_reserved: 0,
        flags_ex: 0,
    };

    let accepted = unsafe {
        if save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        }
    };
    if accepted == 0 {
        return None;
    }
    let len = file_buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(file_buffer.len());
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&file_buffer[..len])))
}

/// `显示名\0*.ext;*.ext\0...\0\0` 形式的过滤器。
fn build_filter(options: &DialogOptions) -> Vec<u16> {
    let mut parts: Vec<u16> = Vec::new();
    for filter in &options.filters {
        parts.extend(filter.name.encode_utf16());
        parts.push(0);
        let patterns = filter
            .extensions
            .iter()
            .map(|extension| format!("*.{extension}"))
            .collect::<Vec<_>>()
            .join(";");
        parts.extend(patterns.encode_utf16());
        parts.push(0);
    }
    if parts.is_empty() {
        // 至少要有一个过滤器，否则类型下拉是空的。
        parts.extend("所有文件".encode_utf16());
        parts.push(0);
        parts.extend("*.*".encode_utf16());
        parts.push(0);
    }
    parts.push(0);
    parts
}

fn wide_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_nul_path(path: &std::path::Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cc_switch_lib::codex_history_content_repair;
use std::ffi::OsString;
use std::io::Write;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Gui,
    ContentPreview { codex_home: Option<String> },
    ContentApply { preview_token: String },
}

/// Parse only the supported offline content-repair commands; never read stdin.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let mut preview = false;
    let mut apply = false;
    let mut codex_home = None;
    let mut preview_token = None;
    let mut any = false;
    while let Some(argument) = args.next() {
        any = true;
        match argument.to_str() {
            Some("--content-preview") if !preview => preview = true,
            Some("--content-apply") if !apply => apply = true,
            Some(flag @ ("--codex-home" | "--preview-token")) => {
                let value = args.next().ok_or_else(|| format!("Missing value for {flag}"))?;
                let value = value
                    .into_string()
                    .map_err(|_| format!("Invalid Unicode value for {flag}"))?;
                if value.trim().is_empty() || value.starts_with("--") {
                    return Err(format!("Missing value for {flag}"));
                }
                let slot = if flag == "--codex-home" {
                    &mut codex_home
                } else {
                    &mut preview_token
                };
                if slot.replace(value).is_some() {
                    return Err(format!("Duplicate argument: {flag}"));
                }
            }
            _ => return Err("Unknown or duplicate argument; supported commands: --content-preview [--codex-home PATH] or --content-apply --preview-token TOKEN".to_string()),
        }
    }
    if !any {
        return Ok(Command::Gui);
    }
    match (preview, apply, codex_home, preview_token) {
        (true, false, codex_home, None) => Ok(Command::ContentPreview { codex_home }),
        (false, true, None, Some(preview_token)) => Ok(Command::ContentApply { preview_token }),
        _ => Err("Use --content-preview [--codex-home PATH] or --content-apply --preview-token TOKEN; do not combine modes or pass --codex-home to apply".to_string()),
    }
}

fn main() {
    let command = parse_args(std::env::args_os().skip(1));
    if matches!(command, Ok(Command::Gui)) {
        #[cfg(target_os = "windows")]
        windows_gui::run();
        #[cfg(not(target_os = "windows"))]
        {
            eprintln!("GUI is available on Windows; use --content-preview [--codex-home PATH] or --content-apply --preview-token TOKEN.");
            std::process::exit(2);
        }
        return;
    }
    prepare_cli_output();
    let outcome = match command {
        Ok(Command::ContentPreview { codex_home }) => {
            codex_history_content_repair::preview(codex_home)
        }
        Ok(Command::ContentApply { preview_token }) => {
            codex_history_content_repair::apply(preview_token)
        }
        Err(error) => {
            eprintln!("Argument error: {error}");
            std::process::exit(2);
        }
        Ok(Command::Gui) => unreachable!(),
    };
    let output = outcome.and_then(|outcome| {
        serde_json::to_vec(&outcome)
            .map_err(|error| format!("Cannot encode repair outcome: {error}"))
    });
    match output {
        Ok(mut output) => {
            output.push(b'\n');
            if let Err(error) = std::io::stdout().lock().write_all(&output) {
                eprintln!("Cannot write repair outcome: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            // Backend errors identify the failed stage without printing command arguments.
            eprintln!("Content repair failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn prepare_cli_output() {}

#[cfg(target_os = "windows")]
fn prepare_cli_output() {
    // A GUI-subsystem executable can still inherit redirected handles. Preserve those;
    // attach to the invoking console only when stdout has no usable inherited handle.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
        fn AttachConsole(process_id: u32) -> i32;
    }
    unsafe {
        let stdout = GetStdHandle(-11i32 as u32);
        if stdout.is_null() || stdout as isize == -1 {
            AttachConsole(u32::MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_preserves_gui_entry() {
        assert_eq!(parse(&[]), Ok(Command::Gui));
    }

    #[test]
    fn preview_accepts_optional_home_in_either_order() {
        assert_eq!(
            parse(&["--content-preview"]),
            Ok(Command::ContentPreview { codex_home: None })
        );
        for args in [
            ["--content-preview", "--codex-home", "C:\\synthetic home"],
            ["--codex-home", "C:\\synthetic home", "--content-preview"],
        ] {
            assert_eq!(
                parse(&args),
                Ok(Command::ContentPreview {
                    codex_home: Some("C:\\synthetic home".to_string())
                })
            );
        }
    }

    #[test]
    fn apply_requires_token_and_rejects_home() {
        assert_eq!(
            parse(&["--content-apply", "--preview-token", "synthetic-token"]),
            Ok(Command::ContentApply {
                preview_token: "synthetic-token".to_string()
            })
        );
        assert!(parse(&["--content-apply"]).is_err());
        assert!(parse(&[
            "--content-apply",
            "--preview-token",
            "token",
            "--codex-home",
            "synthetic"
        ])
        .is_err());
    }

    #[test]
    fn rejects_ambiguous_missing_duplicate_and_unknown_arguments() {
        for args in [
            vec!["--content-preview", "--content-apply"],
            vec!["--content-preview", "--preview-token", "token"],
            vec!["--codex-home", "synthetic"],
            vec!["--preview-token", "token"],
            vec!["--content-preview", "--codex-home"],
            vec!["--content-preview", "--codex-home", ""],
            vec!["--content-apply", "--preview-token", "--content-preview"],
            vec!["--content-preview", "--force"],
            vec!["--content-preview", "extra"],
            vec!["--content-preview", "--content-preview"],
            vec![
                "--content-apply",
                "--content-apply",
                "--preview-token",
                "token",
            ],
            vec![
                "--content-preview",
                "--codex-home",
                "a",
                "--codex-home",
                "b",
            ],
            vec![
                "--content-apply",
                "--preview-token",
                "a",
                "--preview-token",
                "b",
            ],
        ] {
            assert!(
                parse(&args).is_err(),
                "accepted invalid arguments: {args:?}"
            );
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_gui {
    use cc_switch_lib::codex_history_content_repair::{self, ContentRepairOutcome};
    use cc_switch_lib::codex_history_migration::{
        repair_codex_history_visibility_standalone, CodexHistoryStandaloneRepairOptions,
        CodexHistoryVisibilityRepairOutcome,
    };
    use std::ffi::{c_void, OsStr};
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::sync::mpsc::{self, Receiver};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        GetStockObject, UpdateWindow, COLOR_WINDOW, DEFAULT_GUI_FONT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::BST_CHECKED;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW,
        GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, PostMessageW,
        PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
        ShowWindow, TranslateMessage, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
        CREATESTRUCTW, CW_USEDEFAULT, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE,
        ES_READONLY, ES_WANTRETURN, GWLP_USERDATA, HMENU, IDC_ARROW, MSG, SW_SHOW, WM_APP,
        WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT,
        WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    const ID_CODEX_HOME: i32 = 1001;
    const ID_STATE_DB: i32 = 1002;
    const ID_PROJECT_PATH: i32 = 1003;
    const ID_TARGET_PROVIDER: i32 = 1004;
    const ID_SOURCE_PROVIDERS: i32 = 1005;
    const ID_COUNT: i32 = 1006;
    const ID_WINDOW_LIMIT: i32 = 1007;
    const ID_BALANCE_RECENT_WINDOW: i32 = 1014;
    const ID_MAX_PER_PROJECT: i32 = 1015;
    const ID_MAX_TOTAL: i32 = 1016;
    const ID_SOURCE_FILTER: i32 = 1017;
    const ID_INCLUDE_ARCHIVED: i32 = 1008;
    const ID_INCLUDE_SUBAGENTS: i32 = 1009;
    const ID_FORCE: i32 = 1010;
    const ID_PREVIEW: i32 = 1011;
    const ID_APPLY: i32 = 1012;
    const ID_OUTPUT: i32 = 1013;
    const ID_CONTENT_MODE: i32 = 1018;
    const WM_REPAIR_DONE: u32 = WM_APP + 41;
    const LEGACY_INPUT_IDS: &[i32] = &[
        ID_STATE_DB,
        ID_PROJECT_PATH,
        ID_TARGET_PROVIDER,
        ID_SOURCE_PROVIDERS,
        ID_COUNT,
        ID_WINDOW_LIMIT,
        ID_BALANCE_RECENT_WINDOW,
        ID_MAX_PER_PROJECT,
        ID_MAX_TOTAL,
        ID_SOURCE_FILTER,
        ID_INCLUDE_ARCHIVED,
        ID_INCLUDE_SUBAGENTS,
        ID_FORCE,
    ];

    struct ContentPreview {
        token: String,
        affected_files: usize,
        native_config: bool,
    }

    enum RepairResult {
        Content {
            preview: bool,
            result: Result<ContentRepairOutcome, String>,
        },
        Visibility(Box<Result<CodexHistoryVisibilityRepairOutcome, String>>),
        Failed(String),
    }

    /// 保存窗口控件句柄，按钮状态和输出区域由主线程统一更新。
    struct GuiState {
        hwnd: HWND,
        preview_button: HWND,
        apply_button: HWND,
        output: HWND,
        busy: bool,
        content_preview: Option<ContentPreview>,
        completion: Option<Receiver<RepairResult>>,
    }

    impl GuiState {
        /// 构造空状态；真实 HWND 会在 `WM_CREATE` 后填入。
        fn new() -> Self {
            Self {
                hwnd: ptr::null_mut(),
                preview_button: ptr::null_mut(),
                apply_button: ptr::null_mut(),
                output: ptr::null_mut(),
                busy: false,
                content_preview: None,
                completion: None,
            }
        }

        /// 创建并排布所有原生 Windows 控件。
        unsafe fn create_controls(&mut self, hwnd: HWND) {
            self.hwnd = hwnd;
            let font = GetStockObject(DEFAULT_GUI_FONT) as WPARAM;
            let codex_home = std::env::var("USERPROFILE")
                .map(|home| format!(r"{home}\.codex"))
                .unwrap_or_else(|_| r"%USERPROFILE%\.codex".to_string());
            let project_path = std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default();

            let content_mode = create_checkbox(
                hwnd,
                ID_CONTENT_MODE,
                "旧对话续聊修复（全历史，含归档、子任务与压缩历史）",
                16,
                16,
                724,
                26,
                font,
            );
            set_checked(content_mode, true);
            create_label(hwnd, "Codex Home", 16, 56, 110, 22, font);
            create_edit(hwnd, ID_CODEX_HOME, &codex_home, 150, 54, 590, 24, font);
            create_label(hwnd, "State DB (可空)", 16, 90, 120, 22, font);
            create_edit(hwnd, ID_STATE_DB, "", 150, 88, 590, 24, font);
            create_label(hwnd, "项目路径", 16, 124, 110, 22, font);
            create_edit(
                hwnd,
                ID_PROJECT_PATH,
                &project_path,
                150,
                122,
                590,
                24,
                font,
            );
            create_label(hwnd, "目标 provider", 16, 158, 120, 22, font);
            create_edit(hwnd, ID_TARGET_PROVIDER, "", 150, 156, 220, 24, font);
            create_label(hwnd, "空值跟随 config.toml", 386, 158, 170, 22, font);
            create_label(hwnd, "source providers", 16, 192, 120, 22, font);
            create_edit(
                hwnd,
                ID_SOURCE_PROVIDERS,
                "openai, custom, codex_model_router_v2, cc_switch_codex_router, codex_model_router",
                150,
                190,
                590,
                24,
                font,
            );
            create_label(hwnd, "置顶数量", 16, 226, 80, 22, font);
            create_edit(hwnd, ID_COUNT, "30", 150, 224, 80, 24, font);
            create_label(hwnd, "窗口范围", 250, 226, 80, 22, font);
            create_edit(hwnd, ID_WINDOW_LIMIT, "80", 330, 224, 80, 24, font);
            create_label(hwnd, "Source", 16, 260, 80, 22, font);
            create_edit(hwnd, ID_SOURCE_FILTER, "vscode", 150, 258, 90, 24, font);
            let balance_checkbox = create_checkbox(
                hwnd,
                ID_BALANCE_RECENT_WINDOW,
                "Balance recent",
                260,
                258,
                130,
                24,
                font,
            );
            set_checked(balance_checkbox, true);
            create_label(hwnd, "Max/project", 408, 260, 86, 22, font);
            create_edit(hwnd, ID_MAX_PER_PROJECT, "10", 500, 258, 48, 24, font);
            create_label(hwnd, "Max total", 562, 260, 74, 22, font);
            create_edit(hwnd, ID_MAX_TOTAL, "300", 640, 258, 64, 24, font);
            create_checkbox(
                hwnd,
                ID_INCLUDE_ARCHIVED,
                "包含归档",
                440,
                224,
                96,
                24,
                font,
            );
            create_checkbox(
                hwnd,
                ID_INCLUDE_SUBAGENTS,
                "包含 subagent",
                540,
                224,
                118,
                24,
                font,
            );
            create_checkbox(
                hwnd,
                ID_FORCE,
                "Codex 运行时强制写入",
                16,
                294,
                180,
                24,
                font,
            );

            self.preview_button =
                create_button(hwnd, ID_PREVIEW, "预览修复", 210, 292, 120, 30, font);
            self.apply_button = create_button(hwnd, ID_APPLY, "确认写入", 344, 292, 120, 30, font);
            self.output = create_output(hwnd, ID_OUTPUT, 16, 338, 724, 300, font);
            set_text(
                self.output,
                "默认：旧对话续聊修复（全历史）。只恢复已识别的旧 Router 明文误标；扫描普通及压缩历史、归档和子任务。\r\n先预览，切回官方原生配置并完全退出 Codex 后，再在本窗口确认写入。写入前重新校验计划并备份。\r\n取消上方勾选可使用原有侧边栏 / 索引修复。",
            );
            self.refresh_controls();
        }

        /// 当前勾选的是全历史续聊修复模式。
        unsafe fn content_mode(&self) -> bool {
            is_checked(GetDlgItem(self.hwnd, ID_CONTENT_MODE))
        }

        unsafe fn refresh_controls(&self) {
            let content_mode = self.content_mode();
            for id in LEGACY_INPUT_IDS {
                EnableWindow(
                    GetDlgItem(self.hwnd, *id),
                    (!self.busy && !content_mode) as i32,
                );
            }
            EnableWindow(GetDlgItem(self.hwnd, ID_CODEX_HOME), (!self.busy) as i32);
            EnableWindow(GetDlgItem(self.hwnd, ID_CONTENT_MODE), (!self.busy) as i32);
            EnableWindow(self.preview_button, (!self.busy) as i32);
            let content_ready = self.content_preview.as_ref().is_some_and(|preview| {
                preview.native_config && preview.affected_files > 0 && !preview.token.is_empty()
            });
            EnableWindow(
                self.apply_button,
                (!self.busy && (!content_mode || content_ready)) as i32,
            );
        }

        unsafe fn set_busy(&mut self, busy: bool) {
            self.busy = busy;
            self.refresh_controls();
        }
    }

    /// 启动 Windows 消息循环。
    pub fn run() {
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let class_name = wide("CodexHistoryRepairerWindow");
            let window_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: ptr::null_mut(),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                hbrBackground: (COLOR_WINDOW + 1) as usize as _,
                lpszMenuName: ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&window_class);

            let state = Box::new(GuiState::new());
            let state_ptr = Box::into_raw(state);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("Codex 历史修复工具").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                780,
                700,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                state_ptr.cast::<c_void>(),
            );
            if hwnd.is_null() {
                let _ = Box::from_raw(state_ptr);
                return;
            }
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);

            let mut message = MSG::default();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    /// Windows 窗口过程，负责创建控件、响应按钮和接收后台修复结果。
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                1
            }
            WM_CREATE => {
                if let Some(state) = state_from_hwnd(hwnd) {
                    state.create_controls(hwnd);
                }
                0
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as i32;
                let notification = (wparam >> 16) & 0xffff;
                if let Some(state) = state_from_hwnd(hwnd) {
                    if command_id == ID_CONTENT_MODE
                        || (command_id == ID_CODEX_HOME && notification == 0x0300)
                    // EN_CHANGE
                    {
                        state.content_preview = None;
                        state.refresh_controls();
                        if command_id == ID_CONTENT_MODE {
                            set_text(
                                state.output,
                                if state.content_mode() {
                                    "旧对话续聊修复（全历史）：先预览；官方原生配置且 Codex 完全退出后才能写入。旧侧边栏参数不参与此模式。"
                                } else {
                                    "侧边栏 / 索引修复：先预览 active DB、provider、session_index、workspace hints 和 focus 计数，再确认写入。"
                                },
                            );
                        }
                    } else if command_id == ID_PREVIEW || command_id == ID_APPLY {
                        start_repair(state, command_id == ID_PREVIEW);
                    }
                }
                0
            }
            WM_REPAIR_DONE => {
                if let Some(state) = state_from_hwnd(hwnd) {
                    if let Some(result) = state
                        .completion
                        .take()
                        .and_then(|receiver| receiver.try_recv().ok())
                    {
                        let output = match result {
                            RepairResult::Content { preview, result } => {
                                state.content_preview = None;
                                match result {
                                    Ok(outcome) => {
                                        if preview {
                                            state.content_preview = outcome.preview_token.as_ref().map(|token| ContentPreview {
                                                token: token.clone(),
                                                affected_files: outcome.affected_files,
                                                native_config: outcome.native_config,
                                            });
                                        }
                                        format_content_outcome(&outcome)
                                    }
                                    Err(error) => format!("旧对话续聊修复失败（{}）：{error}\r\n执行需要官方原生配置且 Codex 完全退出；处理原因后请重新预览。", if preview { "预览扫描" } else { "执行：校验 / 备份 / 写入 / 读回验证" }),
                                }
                            }
                            RepairResult::Visibility(result) => match *result {
                                Ok(outcome) => format_outcome(&outcome),
                                Err(error) => format!("侧边栏 / 索引修复失败：{error}"),
                            },
                            RepairResult::Failed(error) => {
                                state.content_preview = None;
                                error
                            }
                        };
                        set_text(state.output, &output);
                        state.set_busy(false);
                    }
                }
                0
            }
            WM_CLOSE => {
                if state_from_hwnd(hwnd).is_some_and(|state| state.busy) {
                    // The window owns the result receiver until its worker has completed.
                    return 0;
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_NCDESTROY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
                if !ptr.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(ptr));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    /// 从窗口用户数据里取回 GUI 状态。
    unsafe fn state_from_hwnd(hwnd: HWND) -> Option<&'static mut GuiState> {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
        (!ptr.is_null()).then(|| &mut *ptr)
    }

    /// 收集表单参数；仅允许本窗口成功预览过的续聊计划执行写入。
    unsafe fn start_repair(state: &mut GuiState, dry_run: bool) {
        if state.busy {
            return;
        }
        let content_mode = state.content_mode();
        let token = if content_mode && !dry_run {
            match &state.content_preview {
                Some(preview) if preview.native_config && preview.affected_files > 0 => {
                    Some(preview.token.clone())
                }
                _ => {
                    set_text(
                        state.output,
                        "请先在本窗口成功预览；官方原生配置且存在待修复内容时才能写入。",
                    );
                    return;
                }
            }
        } else {
            None
        };
        let codex_home = empty_to_none(get_text(GetDlgItem(state.hwnd, ID_CODEX_HOME)));
        let options = (!content_mode).then(|| visibility_options(state.hwnd, dry_run));
        state.content_preview = None;
        state.set_busy(true);
        set_text(
            state.output,
            if dry_run {
                "正在预览扫描...（处理期间请保持窗口打开，结果仍可复制）"
            } else {
                "正在执行：校验计划与退出状态 → 完整备份 → 写入 → 读回验证。请保持窗口打开。"
            },
        );
        let hwnd_value = state.hwnd as isize;
        let (sender, receiver) = mpsc::channel();
        state.completion = Some(receiver);
        let worker = std::thread::Builder::new()
            .name("codex-history-repair".to_string())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if content_mode {
                        RepairResult::Content {
                            preview: dry_run,
                            result: match token {
                                Some(token) => codex_history_content_repair::apply(token),
                                None => codex_history_content_repair::preview(codex_home),
                            },
                        }
                    } else {
                        RepairResult::Visibility(Box::new(
                            repair_codex_history_visibility_standalone(
                                options.expect("visibility options"),
                            )
                            .map_err(|error| error.to_string()),
                        ))
                    }
                }))
                .unwrap_or_else(|_| {
                    RepairResult::Failed(
                        "修复工作线程异常退出；请检查备份和后端日志，并重新预览。".to_string(),
                    )
                });
                if sender.send(result).is_ok() {
                    // Results live in the owned channel, never in a raw LPARAM allocation.
                    PostMessageW(hwnd_value as HWND, WM_REPAIR_DONE, 0, 0);
                }
            });
        if let Err(error) = worker {
            state.completion = None;
            set_text(state.output, &format!("无法启动修复线程：{error}"));
            state.set_busy(false);
        }
    }

    unsafe fn visibility_options(hwnd: HWND, dry_run: bool) -> CodexHistoryStandaloneRepairOptions {
        CodexHistoryStandaloneRepairOptions {
            dry_run,
            codex_home: empty_to_none(get_text(GetDlgItem(hwnd, ID_CODEX_HOME))),
            state_db_path: empty_to_none(get_text(GetDlgItem(hwnd, ID_STATE_DB))),
            project_path: empty_to_none(get_text(GetDlgItem(hwnd, ID_PROJECT_PATH))),
            target_provider: empty_to_none(get_text(GetDlgItem(hwnd, ID_TARGET_PROVIDER))),
            source_provider_ids: parse_source_providers(&get_text(GetDlgItem(
                hwnd,
                ID_SOURCE_PROVIDERS,
            ))),
            count: parse_usize(&get_text(GetDlgItem(hwnd, ID_COUNT)), 30),
            window_limit: parse_usize(&get_text(GetDlgItem(hwnd, ID_WINDOW_LIMIT)), 80),
            balance_recent_window: Some(is_checked(GetDlgItem(hwnd, ID_BALANCE_RECENT_WINDOW))),
            max_per_project: parse_usize(&get_text(GetDlgItem(hwnd, ID_MAX_PER_PROJECT)), 10),
            max_total: parse_usize(&get_text(GetDlgItem(hwnd, ID_MAX_TOTAL)), 300),
            source_filter: empty_to_none(get_text(GetDlgItem(hwnd, ID_SOURCE_FILTER))),
            include_archived: Some(is_checked(GetDlgItem(hwnd, ID_INCLUDE_ARCHIVED))),
            include_subagents: Some(is_checked(GetDlgItem(hwnd, ID_INCLUDE_SUBAGENTS))),
            skip_provider_bucket_sync: Some(false),
            force_while_codex_running: Some(is_checked(GetDlgItem(hwnd, ID_FORCE))),
        }
    }

    fn format_content_outcome(result: &ContentRepairOutcome) -> String {
        let mut lines = vec![
            format!(
                "旧对话续聊修复（全历史）：{}",
                if result.dry_run {
                    "只读预览"
                } else {
                    "执行结果"
                }
            ),
            format!("阶段: {}", result.stage),
            format!("Codex Home: {}", result.codex_home),
            "范围: 普通及压缩历史、归档、子任务；仅恢复已识别的旧 Router 明文误标。".to_string(),
            format!("扫描文件: {}", result.scanned_files),
            format!("受影响文件: {}", result.affected_files),
            format!("压缩文件: {}", result.compressed_files),
            format!("恢复正文段: {}", result.repaired_parts),
            format!(
                "官方原生配置: {}",
                if result.native_config {
                    "是"
                } else {
                    "否（请先切回官方，再重新预览）"
                }
            ),
            format!(
                "执行要求 Codex 完全退出: {}",
                if result.requires_codex_exit {
                    "是"
                } else {
                    "否"
                }
            ),
            format!("计划: {}", result.plan_path.as_deref().unwrap_or("未生成")),
            format!(
                "备份: {}",
                result.backup_dir.as_deref().unwrap_or("预览未创建")
            ),
            format!("结果: {}", result.message),
        ];
        if result.dry_run {
            lines.push(if !result.native_config {
                "当前配置不满足执行要求。请切回官方原生配置后，在本窗口重新预览。".to_string()
            } else if result.affected_files == 0 {
                "未发现匹配内容，无需写入。".to_string()
            } else {
                "请完全退出 Codex，再确认写入。目录或模式改变后必须重新预览；历史改变会使计划失效。"
                    .to_string()
            });
        }
        lines.join("\r\n")
    }

    /// 把修复结果格式化为便于复制的多行文本。
    fn format_outcome(result: &CodexHistoryVisibilityRepairOutcome) -> String {
        [
            format!(
                "模式: {}",
                if result.dry_run {
                    "预览"
                } else {
                    "已写入"
                }
            ),
            format!("Codex Home: {}", result.codex_home),
            format!(
                "Active DB: {} ({})",
                result.state_db_path.as_deref().unwrap_or("未找到"),
                result.active_db_kind.as_deref().unwrap_or("-")
            ),
            format!(
                "Live provider: {}",
                result
                    .live_config_model_provider
                    .as_deref()
                    .unwrap_or("未读取到")
            ),
            format!("Target provider: {}", result.target_provider),
            format!(
                "Source providers: {}",
                result.source_provider_ids.join(", ")
            ),
            format!("SQLite threads: {}", result.sqlite_threads),
            format!(
                "Provider rows: {} / {}",
                result.provider_rows_updated, result.provider_rows_to_update
            ),
            format!(
                "Rollout provider lines: {} / {}",
                result.rollout_first_lines_updated, result.rollout_first_lines_to_update
            ),
            format!(
                "has_user_event rows: {} / {}",
                result.user_event_rows_updated, result.user_event_rows_to_update
            ),
            format!(
                "session_index append: {} / {}",
                result.session_index_appended, result.session_index_missing_to_append
            ),
            format!(
                "prefix thread cleanup: {} / {}",
                result.prefix_duplicate_rows_removed, result.prefix_duplicate_rows_to_remove
            ),
            format!(
                "internal transcript cleanup: {} / {}",
                result.internal_transcript_rows_removed, result.internal_transcript_rows_to_remove
            ),
            format!(
                "prefix index cleanup: {} / {}",
                result.session_index_duplicate_rows_removed,
                result.session_index_duplicate_rows_to_remove
            ),
            format!(
                "focus rows: {} / {}",
                result.sqlite_focus_rows_updated, result.sqlite_focus_rows_to_update
            ),
            format!(
                "balanced recent: {} rows / {} projects / max {} per project / max total {}",
                result.balanced_recent_window_rows,
                result.balanced_recent_window_projects,
                result.max_per_project,
                result.max_total
            ),
            format!(
                "session_index move: {} / {}",
                result.session_index_rows_moved, result.session_index_rows_to_move
            ),
            format!(
                "session_index titles: {} / {}",
                result.session_index_titles_updated, result.session_index_titles_to_update
            ),
            format!(
                "workspace hints: {} / {}",
                result.workspace_hints_fixed, result.workspace_hints_to_fix
            ),
            format!(
                "projectless remove: {} / {}",
                result.projectless_ids_removed, result.projectless_ids_to_remove
            ),
            format!(
                "saved roots: {} / {}",
                result.saved_workspace_roots_added, result.saved_workspace_roots_to_add
            ),
            format!(
                "rollout mtimes: {} / {}",
                result.rollout_mtimes_touched, result.rollout_mtimes_to_touch
            ),
            format!(
                "Visible candidates / project window: {} / {}",
                result.visible_candidate_rows, result.visible_project_rows_in_window_before
            ),
            format!(
                "Source filter: {}",
                result
                    .source_filter
                    .as_deref()
                    .unwrap_or("(default cli/vscode)")
            ),
            format!(
                "Backup: {}",
                result.backup_dir.as_deref().unwrap_or("预览模式未创建")
            ),
            format!(
                "Skipped: {}",
                result.skipped_reason.as_deref().unwrap_or("-")
            ),
        ]
        .join("\r\n")
    }

    /// 创建普通标签控件。
    unsafe fn create_label(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32, font: WPARAM) {
        let control = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            hwnd,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
    }

    /// 创建单行输入框。
    // Keep the existing Win32 control geometry signature shared by its call sites.
    #[allow(clippy::too_many_arguments)]
    unsafe fn create_edit(
        hwnd: HWND,
        id: i32,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_LEFT as u32 | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 创建复选框。
    // Keep the existing Win32 control geometry signature shared by its call sites.
    #[allow(clippy::too_many_arguments)]
    unsafe fn create_checkbox(
        hwnd: HWND,
        id: i32,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 设置复选框初始状态，用于默认启用最新的 recent-window 平衡修复。
    unsafe fn set_checked(hwnd: HWND, checked: bool) {
        SendMessageW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::BM_SETCHECK,
            if checked { BST_CHECKED as usize } else { 0 },
            0,
        );
    }

    /// 创建命令按钮。
    // Keep the existing Win32 control geometry signature shared by its call sites.
    #[allow(clippy::too_many_arguments)]
    unsafe fn create_button(
        hwnd: HWND,
        id: i32,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32 | BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 创建多行只读输出框。
    unsafe fn create_output(
        hwnd: HWND,
        id: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32
                | ES_READONLY as u32,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 读取控件文本。
    unsafe fn get_text(hwnd: HWND) -> String {
        let len = GetWindowTextLengthW(hwnd);
        let mut buffer = vec![0u16; len as usize + 1];
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..read as usize])
    }

    /// 写入控件文本。
    unsafe fn set_text(hwnd: HWND, text: &str) {
        SetWindowTextW(hwnd, wide(text).as_ptr());
    }

    /// 判断复选框是否勾选。
    unsafe fn is_checked(hwnd: HWND) -> bool {
        SendMessageW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
            0,
            0,
        ) == BST_CHECKED as isize
    }

    /// 空字符串转为 None。
    fn empty_to_none(value: String) -> Option<String> {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    /// 解析用逗号分隔的 source provider 列表。
    fn parse_source_providers(value: &str) -> Option<Vec<String>> {
        let values = value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        (!values.is_empty()).then_some(values)
    }

    /// 解析正整数，失败时回退到默认值。
    fn parse_usize(value: &str, fallback: usize) -> Option<usize> {
        Some(value.trim().parse::<usize>().unwrap_or(fallback))
    }

    /// 转换 UTF-16 Windows 字符串。
    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }
}

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match cc_switch_lib::codex_config::run_reasoning_cli(&args) {
        Ok(payload) => {
            match serde_json::to_string_pretty(&payload) {
                Ok(json) => println!("{json}"),
                Err(error) => {
                    eprintln!("serialize_error: {error}");
                    return ExitCode::from(2);
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            // stdout 保持纯 JSON 数据；错误和诊断写入 stderr，便于 AI/脚本稳定解析。
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

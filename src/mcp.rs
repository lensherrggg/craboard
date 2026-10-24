//! 极简 MCP stdio 服务：一行一条 JSON-RPC 2.0 消息，stdout 只写协议数据。
//! 工具清单和命令行共用 `core`，不在这里重复业务规则。

use crate::core::{
    AiSummaryView, Core as Workspace, ListOptions, NewTask, SettingsPatch, SettingsView, TaskPatch,
    parse_due_input, parse_repeat, parse_shift, parse_status,
};
use crate::experimental::Experimental;
use crate::state::{Subtask, default_ai_interval_hours};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

pub fn serve_stdio() -> io::Result<()> {
    let (exp, warning) = Experimental::load();
    if let Some(w) = warning {
        eprintln!("craboard: {w}");
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &mut out,
                    json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32700, "message": format!("JSON 解析失败: {e}") }
                    }),
                )?;
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let is_notification = msg.get("id").is_none();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        if is_notification {
            continue;
        }
        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "craboard",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            "ping" => ok_response(id, json!({})),
            "tools/list" => ok_response(id, json!({ "tools": tools(&exp) })),
            "tools/call" => match call_tool(msg.get("params"), &exp) {
                Ok(result) => ok_response(id, result),
                Err(e) => error_response(id, -32602, e),
            },
            other => error_response(id, -32601, format!("未知方法：{other}")),
        };
        write_msg(&mut out, response)?;
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, msg: Value) -> io::Result<()> {
    writeln!(out, "{msg}")?;
    out.flush()
}

fn ok_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn tool_ok(value: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_default()
        }],
        "isError": false
    })
}

fn tool_err(err: impl std::fmt::Display) -> Value {
    json!({
        "content": [{ "type": "text", "text": err.to_string() }],
        "isError": true
    })
}

fn call_tool(params: Option<&Value>, exp: &Experimental) -> std::result::Result<Value, String> {
    let params = params.ok_or_else(|| "缺少 params".to_owned())?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少工具名".to_owned())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(match run_tool(name, &args, exp) {
        Ok(v) => tool_ok(v),
        Err(e) => tool_err(e),
    })
}

fn run_tool(name: &str, args: &Value, exp: &Experimental) -> crate::core::Result<Value> {
    match name {
        "list_tasks" => {
            let ws = Workspace::open()?;
            let opts = ListOptions {
                include_done: arg_bool(args, "include_done").unwrap_or(false),
                important: arg_bool(args, "important"),
                overdue: arg_bool(args, "overdue").unwrap_or(false),
                within_hours: arg_u64(args, "within_hours").map(|v| v as u32),
                text_contains: arg_str(args, "query"),
            };
            let tasks = ws.list(&opts);
            Ok(json!({ "count": tasks.len(), "tasks": tasks }))
        }
        "get_task" => {
            let ws = Workspace::open()?;
            let id = required_id(args)?;
            Ok(json!({ "task": ws.view(id)? }))
        }
        "add_task" => {
            let mut ws = Workspace::open()?;
            let repeat = match args.get("repeat") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => parse_repeat(s)?,
                Some(v) => {
                    return Err(crate::core::CoreError::new(format!(
                        "repeat 需要字符串，收到 {v}"
                    )));
                }
            };
            let input = NewTask {
                text: arg_str(args, "text").unwrap_or_default(),
                due: arg_str(args, "due"),
                important: arg_bool(args, "important").unwrap_or(false),
                repeat,
                subtasks: arg_subtasks(args, "subtasks")?.unwrap_or_default(),
                blocked_by: arg_ids(args, "blocked_by"),
            };
            let task = ws.add(input)?;
            ws.save()?;
            Ok(json!({ "task": task }))
        }
        "update_task" => {
            let mut ws = Workspace::open()?;
            let id = required_id(args)?;
            let patch = TaskPatch {
                text: arg_str(args, "text"),
                due: match args.get("due") {
                    None => None,
                    Some(Value::Null) => Some(None),
                    Some(Value::String(s)) => {
                        let normalized = parse_due_input(s)?;
                        Some(normalized)
                    }
                    Some(v) => {
                        return Err(crate::core::CoreError::new(format!(
                            "due 需要字符串或 null，收到 {v}"
                        )));
                    }
                },
                status: match arg_str(args, "status") {
                    Some(s) => Some(
                        parse_status(&s)
                            .ok_or_else(|| crate::core::CoreError::new(format!("未知状态：{s}")))?,
                    ),
                    None => None,
                },
                important: arg_bool(args, "important"),
                repeat: match args.get("repeat") {
                    None => None,
                    Some(Value::Null) => Some(None),
                    Some(Value::String(s)) => Some(parse_repeat(s)?),
                    Some(v) => {
                        return Err(crate::core::CoreError::new(format!(
                            "repeat 需要字符串或 null，收到 {v}"
                        )));
                    }
                },
                subtasks: arg_subtasks(args, "subtasks")?,
                blocked_by: match args.get("blocked_by") {
                    None => None,
                    Some(Value::Null) => Some(Vec::new()),
                    Some(_) => Some(arg_ids(args, "blocked_by")),
                },
            };
            let outcome = ws.update(id, patch)?;
            ws.save()?;
            Ok(json!({ "task": outcome.task, "next_due": outcome.next_due }))
        }
        "complete_task" => {
            let mut ws = Workspace::open()?;
            let id = required_id(args)?;
            let next = ws.set_status(id, crate::state::TaskStatus::Done)?;
            let task = ws.view(id)?;
            ws.save()?;
            Ok(json!({
                "task": task,
                "next_due": next.map(crate::state::format_due),
                "recurring": next.is_some()
            }))
        }
        "start_task" => {
            let mut ws = Workspace::open()?;
            let id = required_id(args)?;
            ws.set_status(id, crate::state::TaskStatus::Doing)?;
            let task = ws.view(id)?;
            ws.save()?;
            Ok(json!({ "task": task }))
        }
        "set_subtask_done" => {
            let mut ws = Workspace::open()?;
            let id = required_id(args)?;
            let index = required_id_key(args, "index")?;
            let index = usize::try_from(index)
                .ok()
                .filter(|i| *i > 0)
                .map(|i| i - 1)
                .ok_or_else(|| crate::core::CoreError::new("index 从 1 开始"))?;
            let done = arg_bool(args, "done").unwrap_or(true);
            let task = ws.set_subtask_done(id, index, done)?;
            ws.save()?;
            Ok(json!({ "task": task }))
        }
        "postpone_task" => {
            let mut ws = Workspace::open()?;
            let id = required_id(args)?;
            let task = if let Some(to) = args.get("to").and_then(Value::as_str) {
                let due = parse_due_input(to)?
                    .ok_or_else(|| crate::core::CoreError::new("to 不能为空"))?;
                let due = crate::state::parse_due(&due)
                    .ok_or_else(|| crate::core::CoreError::new("to 时间格式不正确"))?;
                ws.postpone_to(id, due)?
            } else {
                let by = args.get("by").and_then(Value::as_str).unwrap_or("1d");
                ws.postpone_by(id, parse_shift(by)?)?
            };
            ws.save()?;
            Ok(json!({ "task": task }))
        }
        "delete_task" => {
            let mut ws = Workspace::open()?;
            let id = required_id(args)?;
            let task = ws.remove(id)?;
            ws.save()?;
            Ok(json!({ "deleted": task }))
        }
        "get_settings" => {
            let ws = Workspace::open()?;
            Ok(json!({ "settings": settings_json(exp, ws.settings_view()) }))
        }
        "update_settings" => {
            check_summary_fields(args, exp)?;
            let mut ws = Workspace::open()?;
            let patch = SettingsPatch {
                show_seconds: arg_bool(args, "show_seconds"),
                reminders_enabled: arg_bool(args, "reminders_enabled"),
                reminder_seconds: arg_u64(args, "reminder_seconds").map(|v| v as u32),
                wip_limit: arg_u64(args, "wip_limit").map(|v| v as usize),
                urgent_within_hours: arg_u64(args, "urgent_within_hours").map(|v| v as u32),
                ai_summary_enabled: arg_bool(args, "ai_summary_enabled"),
                ai_summary_daily_enabled: arg_bool(args, "ai_summary_daily_enabled"),
                ai_summary_daily_times: arg_string_list(args, "ai_summary_daily_times"),
                ai_summary_interval_enabled: arg_bool(args, "ai_summary_interval_enabled"),
                ai_summary_interval_hours: arg_u64(args, "ai_summary_interval_hours")
                    .map(|v| v as u32),
            };
            let view = ws.update_settings(patch)?;
            ws.save()?;
            Ok(json!({ "settings": settings_json(exp, view) }))
        }
        "get_ai_summary" => {
            require_summary(exp)?;
            let ws = Workspace::open()?;
            Ok(json!({ "ai_summary": gate_summary(ws.ai_summary_view(), exp) }))
        }
        "set_ai_summary" => {
            require_summary(exp)?;
            let mut ws = Workspace::open()?;
            let text = arg_str(args, "text")
                .ok_or_else(|| crate::core::CoreError::new("缺少有效的 text"))?;
            let generated_at = arg_str(args, "generated_at");
            let view = ws.set_ai_summary(&text, generated_at.as_deref())?;
            ws.save()?;
            Ok(json!({ "ai_summary": gate_summary(view, exp) }))
        }
        "clear_ai_summary" => {
            require_summary(exp)?;
            let mut ws = Workspace::open()?;
            let view = ws.clear_ai_summary()?;
            ws.save()?;
            Ok(json!({ "ai_summary": gate_summary(view, exp) }))
        }
        other => Err(crate::core::CoreError::new(format!("未知工具：{other}"))),
    }
}

const SUMMARY_SETTING_FIELDS: [&str; 5] = [
    "ai_summary_enabled",
    "ai_summary_daily_enabled",
    "ai_summary_daily_times",
    "ai_summary_interval_enabled",
    "ai_summary_interval_hours",
];

fn require_summary(exp: &Experimental) -> crate::core::Result<()> {
    if exp.summary() {
        return Ok(());
    }
    Err(crate::core::CoreError::new(format!(
        "AI 总结是试验功能，当前未开启。请在 {} 中设置 \"ai_summary\": true 后重启。",
        Experimental::path_label()
    )))
}

fn check_summary_fields(args: &Value, exp: &Experimental) -> crate::core::Result<()> {
    let wants_summary = SUMMARY_SETTING_FIELDS
        .iter()
        .any(|key| args.get(*key).is_some());
    if !wants_summary || exp.active() {
        return Ok(());
    }
    Err(crate::core::CoreError::new(format!(
        "即时总结和定时总结是试验功能，当前未开启。请在 {} 中设置 \"ai_summary_active\": true 后重启。",
        Experimental::path_label()
    )))
}

fn settings_json(exp: &Experimental, view: SettingsView) -> Value {
    let mut value = serde_json::to_value(&view).unwrap_or_else(|_| json!({}));
    exp.filter_settings_json(&mut value);
    value
}

/// 主动总结关闭时，调度字段不应再显示为待更新。
fn gate_summary(mut view: AiSummaryView, exp: &Experimental) -> AiSummaryView {
    if !exp.active() {
        view.enabled = false;
        view.daily_enabled = false;
        view.daily_times.clear();
        view.interval_enabled = false;
        view.interval_hours = default_ai_interval_hours();
        view.stale = false;
        view.next_due_at = None;
        view.window_start = None;
        view.window_end = None;
        view.pending_windows.clear();
    }
    view
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn arg_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn arg_string_list(args: &Value, key: &str) -> Option<Vec<String>> {
    let value = args.get(key)?;
    match value {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        Value::String(raw) => {
            let raw = raw.trim();
            if matches!(
                raw.to_ascii_lowercase().as_str(),
                "" | "none" | "off" | "clear"
            ) {
                return Some(Vec::new());
            }
            Some(
                raw.split([',', '，'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            )
        }
        _ => None,
    }
}

/// 缺失 = 保持原样，null = 清空；其余返回待写入的清单，空行由 Core 统一清洗。
fn arg_subtasks(args: &Value, key: &str) -> crate::core::Result<Option<Vec<Subtask>>> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let items: Vec<Value> = match value {
        Value::Null => return Ok(Some(Vec::new())),
        Value::Array(items) => items.clone(),
        Value::String(raw) => raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Value::String(line.to_owned()))
            .collect(),
        other => {
            return Err(crate::core::CoreError::new(format!(
                "subtasks 需要数组或换行分隔字符串，收到 {other}"
            )));
        }
    };
    items
        .into_iter()
        .map(|item| match item {
            Value::String(text) => Ok(Subtask { text, done: false }),
            Value::Object(map) => {
                let text = map
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| crate::core::CoreError::new("子项对象缺少 text"))?;
                let done = map.get("done").and_then(Value::as_bool).unwrap_or(false);
                Ok(Subtask {
                    text: text.to_owned(),
                    done,
                })
            }
            other => Err(crate::core::CoreError::new(format!(
                "子项需要字符串或 {{text, done}} 对象，收到 {other}"
            ))),
        })
        .collect::<crate::core::Result<Vec<_>>>()
        .map(Some)
}

fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

fn required_id(args: &Value) -> crate::core::Result<u64> {
    required_id_key(args, "id")
}

fn required_id_key(args: &Value, key: &str) -> crate::core::Result<u64> {
    arg_u64(args, key).ok_or_else(|| crate::core::CoreError::new(format!("缺少有效的 {key}")))
}

fn arg_ids(args: &Value, key: &str) -> Vec<u64> {
    match args.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .collect(),
        Some(Value::String(s)) => s
            .split([',', '，', ' '])
            .filter_map(|p| p.trim().parse().ok())
            .collect(),
        _ => Vec::new(),
    }
}

fn subtasks_schema(description: &str) -> Value {
    json!({
        "type": ["array", "null"],
        "description": description,
        "items": {
            "anyOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "done": { "type": "boolean" }
                    },
                    "required": ["text"]
                }
            ]
        }
    })
}

fn tools(exp: &Experimental) -> Value {
    let mut tools = json!([
        {
            "name": "list_tasks",
            "description": "列出 Craboard 任务。默认只返回未完成的任务，可按重要、逾期、截止窗口和关键字筛选。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_done": { "type": "boolean", "description": "是否包含已完成任务" },
                    "important": { "type": "boolean", "description": "只看重要/不重要任务" },
                    "overdue": { "type": "boolean", "description": "只看已经逾期的任务" },
                    "within_hours": { "type": "integer", "description": "只看未来 N 小时内到期（含逾期）" },
                    "query": { "type": "string", "description": "按任务内容模糊搜索" }
                }
            }
        },
        {
            "name": "get_task",
            "description": "读取单个任务的完整信息，包括周期规则、四象限和依赖计数。",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }
        },
        {
            "name": "add_task",
            "description": "新建任务。可带子项清单。重复规则示例：daily、2w、weekly、monthly；周期任务必须给截止时间，完成本轮后子项自动重置。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "due": { "type": "string", "description": "2026-10-21 或 2026-10-21 18:00；只给日期按 23:59" },
                    "important": { "type": "boolean" },
                    "repeat": { "type": "string", "description": "daily、2w、weekly、monthly 等；不重复可省略" },
                    "subtasks": subtasks_schema("子项清单；字符串表示未勾选，{text, done} 可指定勾选状态。空字符串会被忽略"),
                    "blocked_by": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["text"]
            }
        },
        {
            "name": "update_task",
            "description": "修改任务字段。只传需要改的字段；due=null 清空截止，repeat=null 取消重复。subtasks 传数组会整体替换，null 或 [] 清空；单独勾选某一项请用 set_subtask_done。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "text": { "type": "string" },
                    "due": { "type": ["string", "null"] },
                    "status": { "type": "string", "enum": ["todo", "doing", "done"] },
                    "important": { "type": "boolean" },
                    "repeat": { "type": ["string", "null"] },
                    "subtasks": subtasks_schema("整体替换子项清单；null 或 [] 清空。字符串表示未勾选，{text, done} 可指定勾选状态"),
                    "blocked_by": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["id"]
            }
        },
        {
            "name": "complete_task",
            "description": "完成任务。周期任务会推进到下一轮并保持待办，返回 next_due。",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }
        },
        {
            "name": "start_task",
            "description": "把任务标记为进行中；受进行中上限约束。",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }
        },
        {
            "name": "set_subtask_done",
            "description": "勾选或取消勾选一个子项。index 是任务 subtasks 数组里的序号，从 1 开始；勾完全部子项不会自动完成主任务。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "index": { "type": "integer", "description": "子项序号，从 1 开始" },
                    "done": { "type": "boolean", "description": "true 勾选、false 取消；默认 true" }
                },
                "required": ["id", "index"]
            }
        },
        {
            "name": "postpone_task",
            "description": "推迟任务。传 by（如 1d、2h、1w）或 to（新的截止时间）；周期任务只改本轮，下一轮仍按原计划。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "by": { "type": "string", "description": "如 1d、3d、2h、1w；默认 1d" },
                    "to": { "type": "string", "description": "新的截止时间，优先于 by" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "delete_task",
            "description": "删除任务，并清除其他任务对它的前置引用。子项随任务一起删除。",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }
        },
        {
            "name": "get_settings",
            "description": "读取 Craboard 设置，包括四象限紧急窗口、进行中上限和 AI 总结的每日定点/固定间隔配置。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "update_settings",
            "description": "修改设置；未传的字段保持不变。数值会自动收敛到允许范围，每日定点时间会排序去重。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "show_seconds": { "type": "boolean" },
                    "reminders_enabled": { "type": "boolean" },
                    "reminder_seconds": { "type": "integer", "description": "3-60 秒" },
                    "wip_limit": { "type": "integer", "description": "1-99" },
                    "urgent_within_hours": { "type": "integer", "description": "1-720 小时" },
                    "ai_summary_enabled": { "type": "boolean", "description": "定时总结总开关；关闭后保留最近内容但不再提示待更新" },
                    "ai_summary_daily_enabled": { "type": "boolean", "description": "每日定点模式；每次窗口为当天 00:00 到触发时刻" },
                    "ai_summary_daily_times": {
                        "type": "array",
                        "items": { "type": "string", "description": "HH:MM" },
                        "description": "每日定点时间点，如 [\"09:00\",\"18:00\"]；也可传逗号分隔字符串，none/[] 清空"
                    },
                    "ai_summary_interval_enabled": { "type": "boolean", "description": "固定间隔模式；从上次任意来源的总结开始计时" },
                    "ai_summary_interval_hours": { "type": "integer", "description": "固定间隔 1-168 小时；到时悬浮页提示总结待更新" }
                }
            }
        },
        {
            "name": "get_ai_summary",
            "description": "读取今日总结状态：两种定时模式配置、是否可用、来源（ai/instant）、正文、生成时间、是否待更新、下次触发时间、待更新窗口与下一窗口。每日定点窗口从当天 00:00 起；固定间隔窗口从上次总结起。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "set_ai_summary",
            "description": "写入 AI 生成的定时总结，悬浮页会立即改为显示该内容并重置固定间隔计时；来源记为 ai。generated_at 省略时使用当前时间。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "总结正文，最多 4000 字；支持换行" },
                    "generated_at": { "type": "string", "description": "生成时间，如 2026-10-20 19:30；省略用当前时间" }
                },
                "required": ["text"]
            }
        },
        {
            "name": "clear_ai_summary",
            "description": "清除已保存的 AI 总结，悬浮页回到「AI 总结暂不可用」状态。",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ]);
    let list = tools.as_array_mut().expect("工具清单是数组");
    if !exp.summary() {
        list.retain(|tool| {
            !matches!(
                tool["name"].as_str(),
                Some("get_ai_summary" | "set_ai_summary" | "clear_ai_summary")
            )
        });
    }
    if let Some(get) = list.iter_mut().find(|tool| tool["name"] == "get_settings") {
        get["description"] = if exp.active() {
            json!(
                "读取 Craboard 设置，包括四象限紧急窗口、进行中上限，以及 AI 总结的每日定点/固定间隔配置。"
            )
        } else if exp.summary() {
            json!("读取 Craboard 设置，包括四象限紧急窗口、进行中上限和 AI 总结可用状态。")
        } else {
            json!("读取 Craboard 设置，包括四象限紧急窗口和进行中上限。")
        };
    }
    if let Some(update) = list
        .iter_mut()
        .find(|tool| tool["name"] == "update_settings")
    {
        if exp.active() {
            update["description"] = json!(
                "修改设置；未传的字段保持不变。数值会自动收敛到允许范围，每日定点时间会排序去重。"
            );
        } else {
            update["description"] =
                json!("修改设置；未传的字段保持不变。数值会自动收敛到允许范围。");
            if let Some(props) = update["inputSchema"]["properties"].as_object_mut() {
                for key in SUMMARY_SETTING_FIELDS {
                    props.remove(key);
                }
            }
        }
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtasks_accept_strings_objects_lines_and_clear() {
        let args = json!({
            "subtasks": [" 一 ", { "text": "二", "done": true }, ""]
        });
        let parsed = arg_subtasks(&args, "subtasks").unwrap().unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0],
            Subtask {
                text: " 一 ".into(),
                done: false
            }
        );
        assert!(parsed[1].done);

        let lines = json!({ "subtasks": "A\n\n B " });
        let parsed = arg_subtasks(&lines, "subtasks").unwrap().unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].text, " B ");

        assert!(
            arg_subtasks(&json!({ "subtasks": null }), "subtasks")
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert!(arg_subtasks(&json!({}), "subtasks").unwrap().is_none());
        assert!(arg_subtasks(&json!({ "subtasks": 3 }), "subtasks").is_err());
        assert!(arg_subtasks(&json!({ "subtasks": [3] }), "subtasks").is_err());
        assert!(arg_subtasks(&json!({ "subtasks": [{}] }), "subtasks").is_err());
    }

    #[test]
    fn schemas_expose_subtask_editing_and_checking() {
        let tools = tools(&Experimental::default());
        let list = tools.as_array().unwrap();
        let find = |name: &str| list.iter().find(|t| t["name"] == name).unwrap();

        assert!(find("set_subtask_done")["inputSchema"]["properties"]["index"].is_object());
        for name in ["add_task", "update_task"] {
            assert!(find(name)["inputSchema"]["properties"]["subtasks"].is_object());
        }
        let update_desc = find("update_task")["description"].as_str().unwrap();
        assert!(!update_desc.contains("图形界面"));
        assert!(update_desc.contains("set_subtask_done"));
    }

    #[test]
    fn tools_follow_experimental_gates() {
        let count = |exp: &Experimental| {
            tools(exp)
                .as_array()
                .unwrap()
                .iter()
                .filter(|t| {
                    matches!(
                        t["name"].as_str(),
                        Some("get_ai_summary" | "set_ai_summary" | "clear_ai_summary")
                    )
                })
                .count()
        };
        let has_summary_setting = |exp: &Experimental| {
            let tools = tools(exp);
            let update = tools
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == "update_settings")
                .unwrap();
            update["inputSchema"]["properties"]
                .get("ai_summary_enabled")
                .is_some()
        };

        let off = Experimental::default();
        assert_eq!(count(&off), 0);
        assert!(!has_summary_setting(&off));

        let push = Experimental {
            ai_summary: true,
            ai_summary_active: false,
        };
        assert_eq!(count(&push), 3);
        assert!(!has_summary_setting(&push));

        let active = Experimental {
            ai_summary: true,
            ai_summary_active: true,
        };
        assert_eq!(count(&active), 3);
        assert!(has_summary_setting(&active));

        let implied = Experimental {
            ai_summary: false,
            ai_summary_active: true,
        };
        assert_eq!(count(&implied), 3);
        assert!(has_summary_setting(&implied));
    }

    #[test]
    fn push_only_masks_schedule_configuration() {
        let view = AiSummaryView {
            enabled: true,
            available: true,
            source: None,
            daily_enabled: true,
            daily_times: vec!["09:00".into(), "18:00".into()],
            interval_enabled: true,
            interval_hours: 12,
            text: Some("旧总结".into()),
            generated_at: Some("2026-10-21 12:00".into()),
            age_minutes: Some(5),
            stale: true,
            next_due_at: Some("2026-10-21 18:00".into()),
            window_start: Some("2026-10-21 12:00".into()),
            window_end: Some("2026-10-21 18:00".into()),
            pending_windows: Vec::new(),
        };
        let gated = gate_summary(
            view,
            &Experimental {
                ai_summary: true,
                ai_summary_active: false,
            },
        );
        assert!(!gated.enabled);
        assert!(!gated.daily_enabled);
        assert!(!gated.interval_enabled);
        assert!(gated.daily_times.is_empty());
        assert_eq!(gated.interval_hours, default_ai_interval_hours());
        assert!(!gated.stale);
        assert!(gated.next_due_at.is_none());
        assert!(gated.window_start.is_none());
        assert!(gated.window_end.is_none());
        assert!(gated.pending_windows.is_empty());
        assert!(gated.available);
        assert_eq!(gated.text.as_deref(), Some("旧总结"));
        assert_eq!(gated.generated_at.as_deref(), Some("2026-10-21 12:00"));
    }

    #[test]
    fn summary_calls_are_rejected_when_gated_off() {
        let err = require_summary(&Experimental::default()).unwrap_err();
        assert!(err.to_string().contains("ai_summary"));

        let push = Experimental {
            ai_summary: true,
            ai_summary_active: false,
        };
        require_summary(&push).unwrap();
        let err = check_summary_fields(&json!({ "ai_summary_enabled": true }), &push).unwrap_err();
        assert!(err.to_string().contains("ai_summary_active"));
        check_summary_fields(
            &json!({ "ai_summary_enabled": true }),
            &Experimental {
                ai_summary: true,
                ai_summary_active: true,
            },
        )
        .unwrap();
        check_summary_fields(&json!({ "show_seconds": true }), &push).unwrap();
    }
}

//! Craboard 命令行：直接复用 `craboard::core`，和 GUI、MCP 使用同一套规则。
//! `craboard-cli mcp` 启动 stdio MCP 服务。

use craboard::core::{
    Core as Workspace, ListOptions, NewTask, SettingsPatch, TaskPatch, TaskView, parse_due_input,
    parse_repeat, parse_shift, parse_status,
};
use craboard::experimental::Experimental;
use craboard::state::{Subtask, TaskStatus, format_due, normalize_clock};
use serde_json::{Value, json};
use std::collections::VecDeque;

type Result<T> = craboard::core::Result<T>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("错误：{e}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<()> {
    let Some(cmd) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    let rest = &args[1..];
    match cmd {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "list" | "ls" => list(rest),
        "show" => show(rest),
        "add" | "new" => add(rest),
        "edit" | "update" => edit(rest),
        "done" | "complete" => set_status_cmd(rest, TaskStatus::Done),
        "start" => set_status_cmd(rest, TaskStatus::Doing),
        "postpone" | "delay" => postpone(rest),
        "remove" | "rm" | "delete" => remove(rest),
        "settings" | "config" => settings(rest),
        "data-dir" => {
            println!("{}", Workspace::open()?.data_dir().display());
            Ok(())
        }
        "mcp" => craboard::mcp::serve_stdio().map_err(|e| e.to_string().into()),
        other => Err(format!("未知命令「{other}」，运行 craboard-cli help 查看用法").into()),
    }
}

fn print_help() {
    let (exp, warning) = Experimental::load();
    if let Some(w) = warning {
        eprintln!("craboard: {w}");
    }
    println!(
        "Craboard 命令行

用法：
  craboard-cli list [选项]                 列出未完成任务
  craboard-cli show <id>                   查看单个任务
  craboard-cli add <内容> [选项]            新建任务
  craboard-cli edit <id> [选项]             修改任务
  craboard-cli done <id>                   完成任务（周期任务推进到下一轮）
  craboard-cli start <id>                  标记为进行中
  craboard-cli postpone <id> [选项]         推迟任务
  craboard-cli remove <id>                 删除任务
  craboard-cli settings [选项]              查看或修改设置
  craboard-cli data-dir                    输出数据目录
  craboard-cli mcp                         启动 MCP stdio 服务

公共选项：
  --json                                   输出机器可读 JSON

list 选项：
  --all                包含已完成任务
  --important          只看重要任务
  --not-important      只看不重要任务
  --overdue            只看已逾期任务
  --within <小时>      只看未来 N 小时内到期（含逾期）
  -q, --query <文字>    按内容搜索

add / edit 选项：
  --due <时间>         截止时间，如 2026-10-21 18:00；只写日期按 23:59
  --clear-due          清空截止时间
  --important          标为重要（edit 也可用 --no-important）
  --repeat <规则>      重复规则，如 daily、2w、weekly、monthly
  --no-repeat          取消重复
  --blocked-by <ids>   前置任务，逗号分隔；edit 也可用 --clear-blocked-by
  --subtask <文字>     添加一个未勾选子项，可重复；edit 传入即整体替换
  --subtask-done <文字> 添加一个已勾选子项，可重复；edit 传入即整体替换
  --clear-subtasks     清空全部子项（仅 edit）
  --text <内容>        edit：修改任务内容
  --status <状态>      edit：todo / doing / done
  --check <序号>       edit：勾选第 N 个子项，从 1 开始，可重复
  --uncheck <序号>     edit：取消勾选第 N 个子项

postpone 选项：
  --by <时长>          1d、3d、2h、1w，默认 1d
  --to <时间>          直接指定新的截止时间

settings 选项：
  --show-seconds <on|off>
  --reminders <on|off>
  --reminder-seconds <秒>   3-60
  --wip-limit <个数>        1-99
  --urgent-within-hours <小时>  1-720"
    );
    if exp.active() {
        println!(
            "  --ai-summary <on|off>        启用/关闭定时总结
  --ai-daily <on|off>          每日定点模式
  --ai-daily-times <时间,...>  如 09:00,18:00；none 清空
  --ai-interval <on|off>       固定间隔模式
  --ai-summary-hours <小时>    固定间隔 1-168 小时"
        );
    }
    println!(
        "
示例：
  craboard-cli add 写周报 --due 2026-10-21 18:00 --important
  craboard-cli add 发布 1.2 --subtask 打标签 --subtask-done 回归测试
  craboard-cli add 每周复盘 --due 2026-10-23 20:00 --repeat weekly --subtask 收集数据
  craboard-cli edit 3 --subtask 收尾 --subtask-done 复核
  craboard-cli edit 3 --check 2
  craboard-cli list --within 24 --json
  craboard-cli postpone 4 --by 2d
  craboard-cli done 7"
    );
}

struct Parser {
    items: VecDeque<String>,
}

impl Parser {
    fn new(args: &[String]) -> Self {
        Self {
            items: args.iter().cloned().collect(),
        }
    }

    fn next(&mut self) -> Option<String> {
        self.items.pop_front()
    }

    fn empty(&self) -> bool {
        self.items.is_empty()
    }

    fn value(&mut self, flag: &str) -> Result<String> {
        match self.items.pop_front() {
            Some(v) if !v.starts_with("--") => Ok(v),
            _ => Err(format!("{flag} 缺少值").into()),
        }
    }

    fn number(&mut self, flag: &str) -> Result<u64> {
        let raw = self.value(flag)?;
        raw.trim()
            .parse()
            .map_err(|_| format!("{flag} 需要数字，收到「{raw}」").into())
    }
}

fn list(args: &[String]) -> Result<()> {
    let mut p = Parser::new(args);
    let mut opts = ListOptions::default();
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--all" => opts.include_done = true,
            "--important" => opts.important = Some(true),
            "--not-important" => opts.important = Some(false),
            "--overdue" => opts.overdue = true,
            "--within" => opts.within_hours = Some(p.number("--within")?.min(8760) as u32),
            "-q" | "--query" => opts.text_contains = Some(p.value("--query")?),
            "--json" => json_out = true,
            other => return Err(format!("list 不认识的选项「{other}」").into()),
        }
    }
    let ws = Workspace::open()?;
    let tasks = ws.list(&opts);
    if json_out {
        print_json(&json!({ "count": tasks.len(), "tasks": tasks }))?;
    } else if tasks.is_empty() {
        println!("没有符合条件的任务");
    } else {
        for task in tasks {
            println!("{}", task.line());
        }
    }
    Ok(())
}

fn show(args: &[String]) -> Result<()> {
    let mut p = Parser::new(args);
    let id = required_id(&mut p)?;
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--json" => json_out = true,
            other => return Err(format!("show 不认识的选项「{other}」").into()),
        }
    }
    let ws = Workspace::open()?;
    let task = ws.view(id)?;
    if json_out {
        print_json(&json!({ "task": task }))?;
    } else {
        print_task_detail(&task);
    }
    Ok(())
}

fn add(args: &[String]) -> Result<()> {
    let mut p = Parser::new(args);
    let mut text_parts = Vec::new();
    let mut input = NewTask::default();
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--due" => input.due = Some(p.value("--due")?),
            "--important" => input.important = true,
            "--repeat" => input.repeat = parse_repeat(&p.value("--repeat")?)?,
            "--subtask" => input.subtasks.push(Subtask {
                text: p.value("--subtask")?,
                done: false,
            }),
            "--subtask-done" => input.subtasks.push(Subtask {
                text: p.value("--subtask-done")?,
                done: true,
            }),
            "--blocked-by" => input.blocked_by = parse_ids(&p.value("--blocked-by")?),
            "--json" => json_out = true,
            "--" => {
                text_parts.extend(p.items.drain(..));
                break;
            }
            other if other.starts_with("--") => {
                return Err(format!("add 不认识的选项「{other}」").into());
            }
            other => text_parts.push(other.to_owned()),
        }
    }
    input.text = text_parts.join(" ");
    let mut ws = Workspace::open()?;
    let task = ws.add(input)?;
    ws.save()?;
    if json_out {
        print_json(&json!({ "task": task }))?;
    } else {
        println!("已添加 {}", task.line());
    }
    Ok(())
}

fn edit(args: &[String]) -> Result<()> {
    let mut p = Parser::new(args);
    let id = required_id(&mut p)?;
    let mut patch = TaskPatch::default();
    let mut patch_changed = false;
    let mut subtasks = Vec::new();
    let mut replace_subtasks = false;
    let mut clear_subtasks = false;
    let mut subtask_ops: Vec<(usize, bool)> = Vec::new();
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--text" => {
                patch.text = Some(p.value("--text")?);
                patch_changed = true;
            }
            "--due" => {
                let raw = p.value("--due")?;
                patch.due = Some(if raw.eq_ignore_ascii_case("none") {
                    None
                } else {
                    parse_due_input(&raw)?
                });
                patch_changed = true;
            }
            "--clear-due" => {
                patch.due = Some(None);
                patch_changed = true;
            }
            "--status" => {
                let raw = p.value("--status")?;
                patch.status = Some(
                    parse_status(&raw)
                        .ok_or_else(|| format!("未知状态「{raw}」，可用 todo/doing/done"))?,
                );
                patch_changed = true;
            }
            "--important" => {
                patch.important = Some(true);
                patch_changed = true;
            }
            "--no-important" => {
                patch.important = Some(false);
                patch_changed = true;
            }
            "--repeat" => {
                patch.repeat = Some(parse_repeat(&p.value("--repeat")?)?);
                patch_changed = true;
            }
            "--no-repeat" => {
                patch.repeat = Some(None);
                patch_changed = true;
            }
            "--blocked-by" => {
                patch.blocked_by = Some(parse_ids(&p.value("--blocked-by")?));
                patch_changed = true;
            }
            "--clear-blocked-by" => {
                patch.blocked_by = Some(Vec::new());
                patch_changed = true;
            }
            "--subtask" => {
                subtasks.push(Subtask {
                    text: p.value("--subtask")?,
                    done: false,
                });
                replace_subtasks = true;
            }
            "--subtask-done" => {
                subtasks.push(Subtask {
                    text: p.value("--subtask-done")?,
                    done: true,
                });
                replace_subtasks = true;
            }
            "--clear-subtasks" => clear_subtasks = true,
            "--check" => subtask_ops.push((p.number("--check")? as usize, true)),
            "--uncheck" => subtask_ops.push((p.number("--uncheck")? as usize, false)),
            "--json" => json_out = true,
            other => return Err(format!("edit 不认识的选项「{other}」").into()),
        }
    }
    if clear_subtasks && replace_subtasks {
        return Err("--clear-subtasks 不能和 --subtask/--subtask-done 同时使用".into());
    }
    if !subtask_ops.is_empty() && (clear_subtasks || replace_subtasks) {
        return Err("--check/--uncheck 不能和替换子项列表同时使用".into());
    }
    if !subtask_ops.is_empty() && patch.status == Some(TaskStatus::Done) {
        return Err("--check/--uncheck 不能和 --status done 同时使用".into());
    }
    if clear_subtasks {
        patch.subtasks = Some(Vec::new());
        patch_changed = true;
    } else if replace_subtasks {
        patch.subtasks = Some(subtasks);
        patch_changed = true;
    }
    if !patch_changed && subtask_ops.is_empty() {
        return Err("没有指定要修改的内容".into());
    }
    let mut ws = Workspace::open()?;
    let mut next_due = None;
    let mut task = if patch_changed {
        let outcome = ws.update(id, patch)?;
        next_due = outcome.next_due;
        outcome.task
    } else {
        ws.view(id)?
    };
    for (index, done) in subtask_ops {
        let index = index
            .checked_sub(1)
            .ok_or_else(|| "子项序号从 1 开始".to_owned())?;
        task = ws.set_subtask_done(id, index, done)?;
    }
    ws.save()?;
    if json_out {
        print_json(&json!({
            "task": task,
            "next_due": next_due
        }))?;
    } else {
        println!("已更新 {}", task.line());
        if let Some(next) = next_due {
            println!("周期任务已完成本轮，下次 {next}");
        }
    }
    Ok(())
}

fn set_status_cmd(args: &[String], status: TaskStatus) -> Result<()> {
    let mut p = Parser::new(args);
    let id = required_id(&mut p)?;
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--json" => json_out = true,
            other => return Err(format!("不认识的选项「{other}」").into()),
        }
    }
    let mut ws = Workspace::open()?;
    let next = ws.set_status(id, status)?;
    let task = ws.view(id)?;
    ws.save()?;
    if json_out {
        print_json(&json!({
            "task": task,
            "next_due": next.map(format_due),
            "recurring": next.is_some()
        }))?;
    } else if let Some(next) = next {
        println!("已完成本轮 {}；下次 {}", task.line(), format_due(next));
    } else if status == TaskStatus::Done {
        println!("已完成 {}", task.line());
    } else if status == TaskStatus::Doing {
        println!("已开始 {}", task.line());
    } else {
        println!("已更新 {}", task.line());
    }
    Ok(())
}

fn postpone(args: &[String]) -> Result<()> {
    let mut p = Parser::new(args);
    let id = required_id(&mut p)?;
    let mut by = None;
    let mut to = None;
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--by" => by = Some(p.value("--by")?),
            "--to" => to = Some(p.value("--to")?),
            "--json" => json_out = true,
            other => return Err(format!("postpone 不认识的选项「{other}」").into()),
        }
    }
    let mut ws = Workspace::open()?;
    let task = if let Some(to) = to {
        let normalized = parse_due_input(&to)?.ok_or_else(|| "截止时间不能为空".to_owned())?;
        let due = craboard::state::parse_due(&normalized)
            .ok_or_else(|| "截止时间格式不正确".to_owned())?;
        ws.postpone_to(id, due)?
    } else {
        ws.postpone_by(id, parse_shift(by.as_deref().unwrap_or("1d"))?)?
    };
    ws.save()?;
    if json_out {
        print_json(&json!({ "task": task }))?;
    } else {
        let mut line = format!("已延至 {}", task.due.clone().unwrap_or_default());
        if task.repeat.is_some() {
            line.push_str("（周期任务仅本轮，下一轮仍按原计划）");
        }
        println!("{line}");
    }
    Ok(())
}

fn remove(args: &[String]) -> Result<()> {
    let mut p = Parser::new(args);
    let id = required_id(&mut p)?;
    let mut json_out = false;
    while let Some(tok) = p.next() {
        match tok.as_str() {
            "--json" => json_out = true,
            other => return Err(format!("remove 不认识的选项「{other}」").into()),
        }
    }
    let mut ws = Workspace::open()?;
    let task = ws.remove(id)?;
    ws.save()?;
    if json_out {
        print_json(&json!({ "deleted": task }))?;
    } else {
        println!("已删除 {}", task.line());
    }
    Ok(())
}

fn settings(args: &[String]) -> Result<()> {
    let (exp, warning) = Experimental::load();
    if let Some(w) = warning {
        eprintln!("craboard: {w}");
    }
    let mut p = Parser::new(args);
    let mut patch = SettingsPatch::default();
    let mut changed = false;
    let mut json_out = false;
    while let Some(tok) = p.next() {
        if !exp.active()
            && matches!(
                tok.as_str(),
                "--ai-summary"
                    | "--ai-daily"
                    | "--ai-daily-times"
                    | "--ai-interval"
                    | "--ai-summary-hours"
            )
        {
            return Err(active_summary_disabled().into());
        }
        match tok.as_str() {
            "--show-seconds" => {
                patch.show_seconds = Some(parse_on_off(&p.value("--show-seconds")?)?);
                changed = true;
            }
            "--reminders" => {
                patch.reminders_enabled = Some(parse_on_off(&p.value("--reminders")?)?);
                changed = true;
            }
            "--reminder-seconds" => {
                patch.reminder_seconds = Some(p.number("--reminder-seconds")? as u32);
                changed = true;
            }
            "--wip-limit" => {
                patch.wip_limit = Some(p.number("--wip-limit")? as usize);
                changed = true;
            }
            "--urgent-within-hours" => {
                patch.urgent_within_hours = Some(p.number("--urgent-within-hours")? as u32);
                changed = true;
            }
            "--ai-summary" => {
                patch.ai_summary_enabled = Some(parse_on_off(&p.value("--ai-summary")?)?);
                changed = true;
            }
            "--ai-daily" => {
                patch.ai_summary_daily_enabled = Some(parse_on_off(&p.value("--ai-daily")?)?);
                changed = true;
            }
            "--ai-daily-times" => {
                patch.ai_summary_daily_times =
                    Some(parse_clock_list(&p.value("--ai-daily-times")?)?);
                changed = true;
            }
            "--ai-interval" => {
                patch.ai_summary_interval_enabled = Some(parse_on_off(&p.value("--ai-interval")?)?);
                changed = true;
            }
            "--ai-summary-hours" => {
                patch.ai_summary_interval_hours = Some(p.number("--ai-summary-hours")? as u32);
                changed = true;
            }
            "--json" => json_out = true,
            other => return Err(format!("settings 不认识的选项「{other}」").into()),
        }
    }
    let mut ws = Workspace::open()?;
    let view = if changed {
        let view = ws.update_settings(patch)?;
        ws.save()?;
        view
    } else {
        ws.settings_view()
    };
    if json_out {
        let mut value = serde_json::to_value(&view).unwrap_or_else(|_| json!({}));
        exp.filter_settings_json(&mut value);
        print_json(&json!({ "settings": value }))?;
    } else {
        println!("时钟显示秒：{}", on_off(view.show_seconds));
        println!("启用截止提醒：{}", on_off(view.reminders_enabled));
        println!("提醒停留：{} 秒", view.reminder_seconds);
        println!("进行中上限：{} 个", view.wip_limit);
        println!("紧急判定窗口：{} 小时", view.urgent_within_hours);
        if exp.active() {
            println!("定时总结：{}", on_off(view.ai_summary_enabled));
            println!(
                "每日定点：{}",
                if view.ai_summary_daily_enabled {
                    if view.ai_summary_daily_times.is_empty() {
                        "开（未设置时间点）".to_owned()
                    } else {
                        format!("开 · {}", view.ai_summary_daily_times.join(", "))
                    }
                } else {
                    "关".to_owned()
                }
            );
            println!(
                "固定间隔：{}",
                if view.ai_summary_interval_enabled {
                    format!("开 · 每 {} 小时", view.ai_summary_interval_hours)
                } else {
                    "关".to_owned()
                }
            );
        }
        if exp.summary() {
            println!(
                "AI 总结：{}{}",
                if view.ai_summary_available {
                    "可用"
                } else {
                    "暂不可用"
                },
                if exp.active() {
                    ""
                } else {
                    "（仅 MCP 推送）"
                }
            );
        }
        println!("数据目录：{}", view.data_dir);
    }
    Ok(())
}

fn active_summary_disabled() -> String {
    format!(
        "即时总结和定时总结是试验功能，当前未开启；请在 {} 中设置 \"ai_summary_active\": true 后重试",
        Experimental::path_label()
    )
}

fn parse_clock_list(raw: &str) -> Result<Vec<String>> {
    let raw = raw.trim();
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "" | "none" | "off" | "clear"
    ) {
        return Ok(Vec::new());
    }
    raw.split([',', '，'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            normalize_clock(s)
                .ok_or_else(|| format!("时间点「{s}」格式应为 HH:MM，例如 09:30").into())
        })
        .collect()
}

fn parse_on_off(raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "开" | "是" => Ok(true),
        "off" | "false" | "no" | "0" | "关" | "否" => Ok(false),
        _ => Err(format!("需要 on/off，收到「{raw}」").into()),
    }
}

fn on_off(v: bool) -> &'static str {
    if v { "开" } else { "关" }
}

fn required_id(p: &mut Parser) -> Result<u64> {
    if p.empty() {
        return Err("缺少任务 id".into());
    }
    p.number("任务 id")
}

fn parse_ids(raw: &str) -> Vec<u64> {
    raw.split([',', '，', ' '])
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

fn print_json(value: &Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .map_err(|e| craboard::core::CoreError::new(e.to_string()))?
    );
    Ok(())
}

fn print_task_detail(t: &TaskView) {
    println!("#{} {}", t.id, t.text);
    println!("状态：{}", t.status_label);
    println!("重要：{}", if t.important { "是" } else { "否" });
    println!("截止：{}", t.due.as_deref().unwrap_or("无"));
    if let Some(repeat) = &t.repeat {
        println!(
            "重复：{}{}",
            repeat,
            t.repeat_anchor
                .as_deref()
                .map(|a| format!("（本轮基准 {a}）"))
                .unwrap_or_default()
        );
    }
    println!("四象限：{}", t.quadrant_label);
    if !t.blocked_by.is_empty() {
        println!(
            "前置：{}",
            t.blocked_by
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    if t.subtask_total > 0 {
        println!("子项：{}/{} 已完成", t.subtask_done, t.subtask_total);
        for (i, subtask) in t.subtasks.iter().enumerate() {
            println!(
                "  {}. [{}] {}",
                i + 1,
                if subtask.done { "x" } else { " " },
                subtask.text
            );
        }
    }
    println!("创建日：{}", t.day);
}

use super::*;
use chrono::{NaiveDate, NaiveDateTime};

fn task(id: u64, text: &str) -> TodoItem {
    TodoItem::new(id, text.to_owned(), "2026-10-20".to_owned())
}

fn state(todos: Vec<TodoItem>) -> Settings {
    let mut st = Settings::default();
    st.next_id = 100;
    st.todos = todos;
    st
}

fn dt(h: u32, m: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 20)
        .unwrap()
        .and_hms_opt(h, m, 0)
        .unwrap()
}

#[test]
fn legacy_done_migrates() {
    let mut st = state(vec![TodoItem {
        id: 7,
        text: "旧任务".into(),
        status: TaskStatus::Todo,
        legacy_done: true,
        due: None,
        legacy_parent: None,
        subtasks: vec![],
        blocked_by: vec![],
        day: "2026-10-20".into(),
        created_at: None,
        updated_at: None,
        completed_at: None,
        reminded_due: None,
        important: false,
        repeat: None,
        repeat_anchor: None,
    }]);
    st.sanitize();
    assert_eq!(st.todos[0].status, TaskStatus::Done);
}

#[test]
fn legacy_kanban_migrates_to_view() {
    let mut st: Settings = serde_json::from_str(r#"{"kanban":true}"#).unwrap();
    st.sanitize();
    assert_eq!(st.view, View::Kanban);

    let mut st: Settings = serde_json::from_str(r#"{"kanban":false}"#).unwrap();
    st.sanitize();
    assert_eq!(st.view, View::All);

    let mut st: Settings = serde_json::from_str(r#"{"view":"Today"}"#).unwrap();
    st.sanitize();
    assert_eq!(st.view, View::Today);
}

#[test]
fn due_accepts_date_and_datetime() {
    assert_eq!(normalize_due(""), Ok(None));
    assert_eq!(
        normalize_due("2026-10-21"),
        Ok(Some("2026-10-21 23:59".to_owned()))
    );
    assert_eq!(
        normalize_due("2026-10-21 18:00"),
        Ok(Some("2026-10-21 18:00".to_owned()))
    );
    assert!(normalize_due("明天").is_err());
}

#[test]
fn status_checks_prerequisites_and_completes_subtasks() {
    let prereq = task(1, "前置");
    let mut t = task(2, "任务");
    t.blocked_by = vec![1];
    t.subtasks = vec![Subtask::new("子项".into())];
    let mut st = state(vec![prereq, t]);
    st.wip_limit = 3;
    let now = dt(10, 0);
    assert!(st.set_status(2, TaskStatus::Doing, now).is_err());
    st.set_status(1, TaskStatus::Done, now).unwrap();
    st.set_status(2, TaskStatus::Doing, now).unwrap();
    // 子项没勾不影响主任务完成，完成时自动勾上
    st.set_status(2, TaskStatus::Done, now).unwrap();
    assert!(st.todos[1].subtasks[0].done);
}

#[test]
fn legacy_children_flatten_into_root_subtasks() {
    let mut st: Settings = serde_json::from_str(
        r#"{"todos":[
                {"id":1,"text":"根","day":"2026-10-20"},
                {"id":2,"text":"子 B","done":true,"day":"2026-10-20","parent":1},
                {"id":3,"text":"孙","day":"2026-10-20","parent":2},
                {"id":4,"text":"孤儿","day":"2026-10-20","parent":99}
            ]}"#,
    )
    .unwrap();
    st.sanitize();
    assert_eq!(st.todos.len(), 2);
    let root = st.task(1).unwrap();
    assert_eq!(
        root.subtasks,
        vec![
            Subtask {
                text: "子 B".into(),
                done: true
            },
            Subtask {
                text: "孙".into(),
                done: false
            },
        ]
    );
    assert!(st.task(3).is_none());
    assert!(st.task(4).is_some());
    assert!(!serde_json::to_string(&st).unwrap().contains("parent"));
}

#[test]
fn recurring_completion_resets_subtasks() {
    let mut t = recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1);
    t.subtasks = vec![
        Subtask {
            text: "A".into(),
            done: true,
        },
        Subtask {
            text: "B".into(),
            done: true,
        },
    ];
    let mut st = state(vec![t]);
    st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
    assert!(st.todos[0].subtasks.iter().all(|s| !s.done));
}

#[test]
fn subtasks_are_trimmed_and_empty_ones_dropped() {
    let mut t = task(1, "A");
    t.subtasks = vec![
        Subtask {
            text: "  写提纲 ".into(),
            done: true,
        },
        Subtask::new("   ".into()),
    ];
    let mut st = state(vec![t]);
    st.sanitize();
    assert_eq!(
        st.todos[0].subtasks,
        vec![Subtask {
            text: "写提纲".into(),
            done: true
        }]
    );
}

#[test]
fn due_reminders_fire_once_and_again_after_reschedule() {
    let mut t = task(1, "到期任务");
    t.due = Some("2026-10-20 10:00".into());
    let mut st = state(vec![t]);

    assert!(st.take_due_reminders(dt(9, 0), dt(9, 59)).is_empty());
    let got = st.take_due_reminders(dt(9, 59), dt(10, 0));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, "到期任务");
    assert!(st.take_due_reminders(dt(10, 0), dt(10, 30)).is_empty());

    st.todos[0].due = Some("2026-10-20 11:00".into());
    assert_eq!(st.take_due_reminders(dt(10, 30), dt(11, 1)).len(), 1);

    st.todos[0].status = TaskStatus::Done;
    st.todos[0].due = Some("2026-10-20 12:00".into());
    assert!(st.take_due_reminders(dt(11, 1), dt(12, 1)).is_empty());
}

#[test]
fn wip_limit_blocks_new_doing() {
    let mut a = task(1, "A");
    a.status = TaskStatus::Doing;
    let b = task(2, "B");
    let mut st = state(vec![a, b]);
    st.wip_limit = 1;
    let now = dt(10, 0);
    assert!(st.set_status(2, TaskStatus::Doing, now).is_err());
    st.wip_limit = 2;
    assert!(st.set_status(2, TaskStatus::Doing, now).is_ok());
}

#[test]
fn dependency_cycle_check_ignores_the_task_itself() {
    let a = task(1, "A");
    let mut b = task(2, "B");
    b.blocked_by = vec![1];
    let mut st = state(vec![a, b]);
    assert!(!st.dep_has_cycle(1));
    assert!(!st.dep_has_cycle(2));

    st.todos[0].blocked_by = vec![2];
    assert!(st.dep_has_cycle(1));
    assert!(st.dep_has_cycle(2));
}

#[test]
fn dependency_cycles_are_detected_and_broken() {
    let mut a = task(1, "A");
    let mut b = task(2, "B");
    a.blocked_by = vec![2];
    b.blocked_by = vec![1];
    let mut st = state(vec![a, b]);
    st.sanitize();
    assert!(st.todos.iter().all(|t| t.blocked_by.is_empty()));
}

#[test]
fn legacy_parent_cycles_stay_top_level() {
    let mut st: Settings = serde_json::from_str(
        r#"{"todos":[
                {"id":1,"text":"A","day":"2026-10-20","parent":2},
                {"id":2,"text":"B","day":"2026-10-20","parent":1}
            ]}"#,
    )
    .unwrap();
    st.sanitize();
    assert_eq!(st.todos.len(), 2);
    assert!(st.todos.iter().all(|t| t.subtasks.is_empty()));
}

#[test]
fn settings_save_omits_tasks_but_still_reads_legacy() {
    let st = state(vec![task(1, "A")]);
    let json = serde_json::to_string(&st).unwrap();
    assert!(!json.contains("todos"));
    assert!(
        serde_json::from_str::<Settings>(&json)
            .unwrap()
            .todos
            .is_empty()
    );

    let mut legacy: Settings = serde_json::from_str(
        r#"{"todos":[{"id":3,"text":"旧任务","done":true,"day":"2026-10-20"}]}"#,
    )
    .unwrap();
    assert!(legacy.todos[0].legacy_done);
    legacy.sanitize();
    assert!(legacy.todos[0].is_done());
}

#[test]
fn duplicate_task_ids_are_dropped() {
    let mut st = state(vec![task(1, "先来的"), task(1, "重复的")]);
    st.sanitize();
    assert_eq!(st.todos.len(), 1);
    assert_eq!(st.todos[0].text, "先来的");
}

#[test]
fn deleting_task_clears_references() {
    let a = task(1, "A");
    let mut b = task(2, "B");
    b.blocked_by = vec![1];
    let mut st = state(vec![a, b]);
    st.remove_task(1);
    assert_eq!(st.todos.len(), 1);
    assert!(st.todos[0].blocked_by.is_empty());
}

fn recurring(id: u64, due: &str, unit: RepeatUnit, interval: u32) -> TodoItem {
    let mut t = task(id, "周期任务");
    t.due = Some(due.to_owned());
    t.repeat = Some(Repeat::new(unit, interval));
    t.repeat_anchor = Some(due.to_owned());
    t
}

#[test]
fn recurring_completion_advances_to_next_round() {
    let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1)]);

    let next = st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
    assert_eq!(next, parse_due("2026-10-21 09:00"));
    let t = &st.todos[0];
    assert_eq!(t.status, TaskStatus::Todo);
    assert_eq!(t.due.as_deref(), Some("2026-10-21 09:00"));
    assert_eq!(t.repeat_anchor.as_deref(), Some("2026-10-21 09:00"));
    assert_eq!(t.completed_at, None);
    assert_eq!(t.updated_at.as_deref(), Some("2026-10-20 08:00"));

    let next = st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
    assert_eq!(next, parse_due("2026-10-22 09:00"));
}

#[test]
fn timestamps_follow_status_and_postpone() {
    let mut st = state(vec![task(1, "A")]);
    st.set_status(1, TaskStatus::Doing, dt(8, 0)).unwrap();
    assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 08:00"));
    assert_eq!(st.todos[0].completed_at, None);

    st.set_status(1, TaskStatus::Done, dt(9, 30)).unwrap();
    assert_eq!(
        st.todos[0].completed_at.as_deref(),
        Some("2026-10-20 09:30")
    );

    st.set_status(1, TaskStatus::Todo, dt(10, 0)).unwrap();
    assert_eq!(st.todos[0].completed_at, None);
    assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 10:00"));

    st.postpone(1, chrono::Duration::days(1), dt(11, 0))
        .unwrap();
    assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 11:00"));
}

#[test]
fn sanitize_backfills_missing_timestamps_from_creation_day() {
    let mut done = task(1, "旧的已完成");
    done.status = TaskStatus::Done;
    let mut st = state(vec![done]);
    st.sanitize();
    assert_eq!(st.todos[0].created_at.as_deref(), Some("2026-10-20 00:00"));
    assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 00:00"));
    // 旧数据没有完成时间，不能拿创建日冒充
    assert_eq!(st.todos[0].completed_at, None);
}

#[test]
fn completing_postponed_round_does_not_repeat_the_same_due() {
    let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1)]);
    st.postpone(1, chrono::Duration::days(1), dt(8, 0)).unwrap();
    assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-21 09:00"));
    assert_eq!(
        st.todos[0].repeat_anchor.as_deref(),
        Some("2026-10-20 09:00")
    );

    let next = st.set_status(1, TaskStatus::Done, dt(8, 30)).unwrap();
    assert_eq!(next, parse_due("2026-10-22 09:00"));
    assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-22 09:00"));
}

#[test]
fn late_completion_skips_missed_rounds() {
    let mut st = state(vec![recurring(1, "2026-10-18 09:00", RepeatUnit::Day, 1)]);
    let next = st.set_status(1, TaskStatus::Done, dt(10, 0)).unwrap();
    assert_eq!(next, parse_due("2026-10-21 09:00"));
}

#[test]
fn recurring_interval_and_units() {
    let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Week, 2)]);
    let next = st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
    assert_eq!(next, parse_due("2026-11-03 09:00"));

    let mut st = state(vec![recurring(1, "2026-10-31 09:00", RepeatUnit::Month, 1)]);
    let now = NaiveDate::from_ymd_opt(2026, 10, 31)
        .unwrap()
        .and_hms_opt(8, 0, 0)
        .unwrap();
    let next = st.set_status(1, TaskStatus::Done, now).unwrap();
    assert_eq!(
        next,
        Some(
            NaiveDate::from_ymd_opt(2026, 11, 30)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap()
        )
    );
}

#[test]
fn postpone_keeps_the_recurring_anchor() {
    let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1)]);
    let now = dt(8, 0);
    let next = st.postpone(1, chrono::Duration::days(1), now).unwrap();
    assert_eq!(next, parse_due("2026-10-21 09:00").unwrap());
    assert_eq!(
        st.todos[0].repeat_anchor.as_deref(),
        Some("2026-10-20 09:00")
    );

    let done_at = NaiveDate::from_ymd_opt(2026, 10, 21)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap();
    let next = st.set_status(1, TaskStatus::Done, done_at).unwrap();
    assert_eq!(next, parse_due("2026-10-22 09:00"));
}

#[test]
fn postpone_moves_overdue_from_now() {
    let mut t = task(1, "逾期任务");
    t.due = Some("2026-10-19 09:00".into());
    let mut st = state(vec![t]);
    let next = st
        .postpone(1, chrono::Duration::days(1), dt(10, 0))
        .unwrap();
    assert_eq!(next, parse_due("2026-10-21 10:00").unwrap());
    assert_eq!(st.todos[0].reminded_due, None);
}

#[test]
fn urgency_uses_threshold_and_ignores_done() {
    let mut soon = task(1, "五小时后");
    soon.due = Some("2026-10-20 15:00".into());
    let mut later = task(2, "两天后");
    later.due = Some("2026-10-22 10:00".into());
    let mut overdue = task(3, "逾期");
    overdue.due = Some("2026-10-19 10:00".into());
    let mut done = task(4, "已完成");
    done.due = Some("2026-10-20 11:00".into());
    done.status = TaskStatus::Done;
    let none = task(5, "无截止");
    let now = dt(10, 0);

    assert!(soon.is_urgent(now, 24));
    assert!(!later.is_urgent(now, 24));
    assert!(overdue.is_urgent(now, 1));
    assert!(!done.is_urgent(now, 24));
    assert!(!none.is_urgent(now, 720));
    assert_eq!(soon.quadrant(now, 24), Quadrant::Urgent);
    assert_eq!(overdue.quadrant(now, 24), Quadrant::Urgent);
}

#[test]
fn repeat_parsing() {
    assert_eq!(Repeat::parse("2w"), Some(Repeat::new(RepeatUnit::Week, 2)));
    assert_eq!(
        Repeat::parse("daily"),
        Some(Repeat::new(RepeatUnit::Day, 1))
    );
    assert_eq!(
        Repeat::parse("每 3 天"),
        Some(Repeat::new(RepeatUnit::Day, 3))
    );
    assert!(Repeat::parse("someday").is_none());
}

#[test]
fn font_size_defaults_small_and_round_trips() {
    let mut st: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(st.font_size, FontSize::Small);
    st.font_size = FontSize::Large;
    let json = serde_json::to_string(&st).unwrap();
    assert!(json.contains("\"font_size\":\"large\""));
    assert_eq!(FontSize::parse("MEDIUM"), Some(FontSize::Medium));
    assert_eq!(FontSize::parse("大"), None);
    assert_eq!(FontSize::ALL.len(), 3);
}

#[test]
fn theme_defaults_warm_night_and_round_trips() {
    let mut st: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(st.theme, ThemeKind::WarmNight);
    st.theme = ThemeKind::WarmPaper;
    let json = serde_json::to_string(&st).unwrap();
    assert!(json.contains("\"theme\":\"warm_paper\""));
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.theme, ThemeKind::WarmPaper);
    assert_eq!(ThemeKind::ALL.len(), 2);
}

#[test]
fn autostart_defaults_false_and_round_trips() {
    let mut st: Settings = serde_json::from_str("{}").unwrap();
    assert!(!st.autostart);
    st.autostart = true;
    let json = serde_json::to_string(&st).unwrap();
    assert!(json.contains("\"autostart\":true"));
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert!(back.autostart);
}

#[test]
fn close_behavior_defaults_to_ask_and_round_trips() {
    let mut st: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(st.close_behavior, CloseBehavior::Ask);
    st.close_behavior = CloseBehavior::Tray;
    let json = serde_json::to_string(&st).unwrap();
    assert!(json.contains("\"close_behavior\":\"tray\""));
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.close_behavior, CloseBehavior::Tray);
    assert_eq!(CloseBehavior::ALL.len(), 3);
}

#[test]
fn settings_clamp_new_options() {
    let mut st: Settings =
        serde_json::from_str(r#"{"urgent_within_hours":0,"reminder_seconds":999}"#).unwrap();
    st.sanitize();
    assert_eq!(st.urgent_within_hours, 1);
    assert_eq!(st.reminder_seconds, 60);
}

#[test]
fn ai_summary_defaults_and_sanitize() {
    let mut st: Settings = serde_json::from_str("{}").unwrap();
    st.sanitize();
    assert!(st.ai_summary_enabled);
    assert!(st.ai_summary_interval_enabled);
    assert_eq!(st.ai_summary_interval_hours, 6);
    assert!(!st.ai_summary_daily_enabled);
    assert!(st.ai_summary_daily_times.is_empty());
    assert!(st.ai_summary.is_none());

    st.ai_summary_daily_times = vec![
        "18:00".into(),
        "9:05".into(),
        "18:00".into(),
        "25:00".into(),
        "早上".into(),
    ];
    st.sanitize();
    assert_eq!(st.ai_summary_daily_times, ["09:05", "18:00"]);

    st.ai_summary_interval_hours = 0;
    st.ai_summary = Some(AiSummary {
        text: "  今日完成 2 项  ".into(),
        generated_at: "2026-10-20 19:30".into(),
        source: SummarySource::Instant,
    });
    st.sanitize();
    assert_eq!(st.ai_summary_interval_hours, 1);
    assert_eq!(st.ai_summary.as_ref().unwrap().text, "今日完成 2 项");
    assert_eq!(
        st.ai_summary.as_ref().unwrap().source,
        SummarySource::Instant
    );

    st.ai_summary_interval_hours = 999;
    st.ai_summary = Some(AiSummary {
        text: "总结".into(),
        generated_at: "不是时间".into(),
        source: SummarySource::Ai,
    });
    st.sanitize();
    assert_eq!(st.ai_summary_interval_hours, 168);
    assert!(st.ai_summary.is_none());
}

#[test]
fn legacy_ai_summary_reads_as_ai_source() {
    let summary: AiSummary =
        serde_json::from_str(r#"{"text":"旧总结","generated_at":"2026-10-20 09:00"}"#).unwrap();
    assert_eq!(summary.source, SummarySource::Ai);
}

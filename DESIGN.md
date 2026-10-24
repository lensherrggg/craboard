# Craboard 设计文档

## 1. 定位与目标

Craboard 是一个 Windows 本地任务看板。设计优先级从高到低：

1. **本地优先**：不依赖网络、账号和外部服务；断网可用，数据可直接备份。
2. **响应快**：窗口空闲时按分钟级唤醒，交互反馈不经过网络。
3. **规则集中**：GUI、CLI、MCP 共用同一套任务规则，不在各入口重复实现。
4. **小体积**：不引入前端运行时、数据库服务和模型 SDK；单个 exe 可运行。

非目标：多人协作、云同步、移动端、富文本编辑器。

## 2. 总体架构

```
┌──────────────────────────── 入口层 ────────────────────────────┐
│  craboard.exe (GUI)          craboard-cli.exe                  │
│  main.rs / ui/*              bin/craboard-cli.rs               │
│       │                            │                           │
│       └──────────┬─────────────────┘                           │
│                  │           mcp.rs (stdio JSON-RPC)           │
│                  ▼                                             │
│            core/ (Core：任务规则、视图投影、调度)               │
│                  │                                             │
│        ┌─────────┴──────────┐                                  │
│        ▼                    ▼                                  │
│   state.rs             storage/                                │
│   数据模型/设置          TaskStore + TaskRepository             │
│   JSON 原子写            SQLite / JSON / Memory                 │
└────────────────────────────────────────────────────────────────┘
```

| 模块 | 职责 |
| --- | --- |
| `state.rs` | `TodoItem`、`Repeat`、`Settings`、`View` 等模型；日期解析；`state.json` 读写与 `sanitize`。 |
| `storage/` | 存储抽象 `TaskStore`、差异同步 `TaskRepository`、SQLite / JSON / 内存三种后端、旧数据迁移。 |
| `core/mod.rs` | `Core`：持有设置与任务，提供增删改、状态迁移、列表/视图投影、总结调度和保存。 |
| `core/edit.rs` | `TaskEditDraft`：新建/编辑弹窗的校验与提交事务，含子项清洗。 |
| `core/view.rs` | 纯函数投影：`TaskInfo`、今天口径、四象限、排序、今日进度、即时总结文本。 |
| `ui/` | egui 界面：board（主框架/列表/四象限）、kanban（看板/编辑弹窗）、settings、reminder、theme。 |
| `main.rs` | GUI 生命周期、托盘、全局快捷键、自动保存、外部改动监听。 |
| `build.rs` | 把 `assets/craboard.ico` 和多尺寸元数据编译成 Windows 资源，链接进 exe。 |
| `win32.rs` | 少量 Win32 调用：窗口查找、显示/隐藏、屏幕边界、DWM 样式、光标/工作区、开机启动项。 |
| `experimental.rs` | 读取 exe 同目录的 `experimental.json`，决定试验功能是否开放，并过滤对外设置字段。 |
| `mcp.rs` | 极简 MCP stdio 服务，工具实现复用 `core`，工具清单按试验开关裁剪。 |
| `bin/craboard-cli.rs` | 命令行解析与输出，复用 `core`，帮助与设置输出按试验开关裁剪。 |

## 3. 关键数据流

### 3.1 一次任务修改（GUI）

```
用户操作 → ui 构造变更（或 TaskEditDraft）
        → Core 校验并修改内存 Settings
        → is_dirty = true
        → 400ms 防抖后 Core::save
             ├─ repo.sync 只写变化的行（SQLite 事务）
             └─ state::save 原子替换 state.json
```

- GUI 不直接写数据库，所有修改先落到内存，再统一保存。
- 任务与设置分别有 dirty 标记；只有发生变化的存储会被写。
- 退出时 `on_exit` 再保存一次。

### 3.2 一次任务修改（CLI / MCP）

```
进程启动 → Core::open（读 state.json + 任务库）
        → 执行一次操作
        → Core::save
        → 进程退出
```

CLI/MCP 是短生命周期进程，每次操作重新打开存储，天然读到最新数据。

### 3.3 外部改动同步

- GUI 后台线程每 900ms 检查任务库、任务兜底 JSON 和 `state.json` 的修改时间。
- 有变化时置位 `watch_flag` 并唤醒主循环。
- 主循环确认本地没有未保存改动后调用 `Core::reload_all`，整体重读设置与任务。
- GUI 自己保存后会刷新时间戳基线，避免把自己的写入当成外部改动。

## 4. 数据模型

### 4.1 任务

```rust
TodoItem {
    id: u64,
    text: String,
    status: TaskStatus,       // todo / doing / done
    due: Option<String>,      // 本地时间 "YYYY-MM-DD HH:MM"
    blocked_by: Vec<u64>,     // 前置任务
    subtasks: Vec<Subtask>,   // 附属勾选清单，不是独立任务
    day: String,              // 创建日 "YYYY-MM-DD"
    created_at: Option<String>,   // 创建时间 "YYYY-MM-DD HH:MM"
    updated_at: Option<String>,   // 最近一次用户可见修改
    completed_at: Option<String>, // 完成时间；取消完成时清空
    reminded_due: Option<String>,
    important: bool,
    repeat: Option<Repeat>,
    repeat_anchor: Option<String>,
}
```

设计要点：

- **ID 由存储层分配**，GUI 和 CLI 并发新增也不会撞号。
- **截止时间是字符串**，统一格式后排序和比较都简单，也便于人工查看；读取时通过 `parse_due` 解析成 `NaiveDateTime`。
- **`day` 是创建日**，用于「今天」里没有截止时间的任务。
- **`subtasks` 是任务内的勾选清单**：`Subtask { text, done }`，只有文本和勾选状态，不设截止、状态、重要性、前置或周期，也不参与任何按任务的统计。
- **旧的父子关系只读迁移**：`TodoItem` 保留一个不序列化的 `legacy_parent`，仅用于在读入旧库时把后代合并进根任务清单；新数据不再写 `parent`。
- **`reminded_due` 记录已提醒的截止值**，改期后清空，保证新的截止时间还能再提醒。
- **`repeat_anchor` 是周期基准**，与 `due` 分离，使「本轮延期」不改变下一轮计划。注意完成周期任务后，基准会推进到新的一轮。
- **时间戳都是可选的本地时间字符串**：新建任务写入 `created_at` / `updated_at`；编辑、改状态、延期只更新 `updated_at`；完成时写 `completed_at`，取消完成时清空。周期任务完成一轮只是推进计划，不设置 `completed_at`。
- **旧数据的时间戳**：`sanitize` 在缺失时用 `day` 补 `created_at`，并以它补 `updated_at`；旧已完成任务的 `completed_at` 无法还原，保持 `None`。

### 4.2 周期规则

```rust
Repeat { unit: Day|Week|Month|Year, interval: u32 }  // interval 1..=999
```

- 解析支持 `daily`、`weekly`、`monthly`、`yearly`，以及 `2d`、`3w` 这类间隔写法；
- 月/年推进使用 `add_months`，处理月末日期；
- `next_after(anchor, now)` 会从基准逐轮推进，跳过已经错过的轮次，避免一次完成只前进一步。
- 周期任务必须有截止时间，这是模型不变式；`sanitize` 和编辑校验都会兜底。
- 周期任务可以带子项：子项只是清单，不参与周期推进。完成一轮时子项全部重置为未勾选，避免下一轮直接显示成已完成。

### 4.3 设置

`Settings` 同时承担界面状态和用户偏好：

- 窗口：位置、大小、右侧便签宽、便签是否展开。
- 外观：`FontSize`（小 1.0 / 中 1.12 / 大 1.25）、`ThemeKind`（暖夜 / 暖纸）。
- 行为：是否显示秒、是否提醒、提醒停留秒数、进行中上限、紧急窗口。
- 启动：是否开机自启；开启后登录 Windows 时自动启动。仅 GUI 修改并写当前用户启动项，不进入 `SettingsView` / `SettingsPatch`。
- 关闭：`CloseBehavior`（`ask` / `tray` / `quit`），决定标题栏 ×、Alt+F4 和任务栏关闭的去向；默认 `ask`。同样只属于 GUI。
- 视图：当前视图、看板收起状态、列表折叠状态。
- 总结：总开关、每日定点时间、固定间隔配置、最近一次总结内容。这些字段默认不进入界面和对外视图，只在 `experimental.json` 开启后按级别暴露：`ai_summary` 开放面板与 MCP 读写，`ai_summary_active` 再开放即时总结和两种定时模式。

`Settings::sanitize` 在每次加载后执行：收敛数值范围、规范化每日时间、清理失效引用、去重 ID、清洗子项、合并旧父子关系、打断前置环、校验周期。它保证「磁盘上的数据再脏，进入内存的模型也是自洽的」。

### 4.4 便签

便签独立于任务：`NoteCard { id, text }`，只存在 `state.json`，可增删、可调宽，不进入任务库。

## 5. 存储层

### 5.1 抽象

```rust
trait TaskStore {
    fn load(&mut self) -> Result<Vec<TodoItem>>;
    fn apply(&mut self, changes: TaskChanges) -> Result<()>;
    fn allocate_id(&mut self) -> Result<u64>;
    fn watch_paths(&self) -> Vec<PathBuf>;   // 内存后端不提供
}
```

`TaskRepository` 在内存里保留一份 `persisted` 快照，`sync` 时对比目标列表，只生成新增/修改的 upsert 和缺失的 delete，避免整表重写。

### 5.2 SQLite 后端（主）

- 文件：`%APPDATA%\craboard\tasks.db`。
- `PRAGMA journal_mode=WAL`、`synchronous=NORMAL`，兼顾可靠性和写入开销。
- `busy_timeout=3s`，缓解 GUI 与 CLI 同时写入时的锁竞争。
- schema 版本写在 `user_version`，当前为 4；程序发现更高版本会拒绝打开，避免旧程序破坏新数据。
- `blocked_by` 以 JSON 数组文本存储；`repeat` 优先按 JSON 解析，兼容旧的纯文本规则；`subtasks` 存 JSON 数组。
- `created_at` / `updated_at` / `completed_at` 为可空 TEXT；v3 迁移时用 `day || ' 00:00'` 回填前两者，`completed_at` 留空。
- `parent` 列只为读取旧数据保留；新的 upsert 一律写 `NULL`，避免旧的父子结构在库里复活。
- 批量变更在一个事务中提交。

### 5.3 降级链

```
SQLite 打开成功 ──→ 使用 tasks.db
      │失败
      ▼
tasks.json 可打开 ──→ 使用 JSON 后端，并记录告警
      │失败
      ▼
内存后端 ──→ 界面仍可用，但退出后数据丢失，日志记录告警
```

降级告警会写入 `crash.log` 并在启动时记录，不会弹窗打断使用。JSON 后端既方便排查，也接受早期的裸数组格式。

### 5.4 迁移

1. **旧 JSON 任务 → SQLite**：首次打开时，若任务库为空而 `state.json` 仍带 `todos`，把任务导入 SQLite；原 `state.json` 复制为 `state.json.before-sqlite`，之后保存不再回写 `todos`。
2. **旧 `done` 布尔 → `TaskStatus`**：读取时映射，写回时不再输出旧字段。
3. **旧 `kanban` 布尔 → `View`**：在 `sanitize` 中迁移一次。
4. **schema v2 → v3**：增加三个时间戳列；旧行用 `day` 回填创建/更新时间，旧任务的完成时间保持未知。
5. **schema v3 → v4**：增加 `subtasks` 列，默认 `[]`。
6. **旧父子任务 → 子项清单**：加载时若任务仍有 `parent`，把所有后代按子任务 ID 顺序展开成根任务的 `Subtask { text, done }`，删除子任务行，并清空旧关系；找不到父任务或形成环的任务保持顶层。启动时检测到旧关系会置脏标记，下一次保存把干净结构写回数据库。
7. **旧 JSON 任务补时间戳**：从 `state.json` 或 `tasks.json` 读到的任务没有时间戳时，`sanitize` 用 `day` 补创建时间，再补更新时间。

### 5.5 设置文件

`state.json` 采用「写临时文件 → 替换」的原子方式，避免崩溃时留下半截 JSON。文件内容是可读的 pretty JSON，便于备份和排错。

## 6. 业务规则

### 6.1 状态迁移

- 开始（doing）：存在未完成前置任务时拒绝；进行中数量达到上限时拒绝。
- 完成（done）：存在未完成前置任务时拒绝；完成后把全部子项勾选。
- 周期任务完成：不进入 done，而是推进到下一轮，状态回到 todo，返回 `next_due`，并把全部子项重置为未勾选。
- 看板拖动同样走 `set_status`，规则一致。

### 6.2 前置依赖与子项

- 前置依赖在编辑时校验：不能引用自己、目标必须存在、不能形成环。
- 子项是任务内部的清单，不建立任务之间的关系，因此没有父子环、挂载限制或截止继承问题。
- 新建任务与编辑任务共用 `TaskEditDraft`：子项文本会 trim、丢弃空行，勾选状态保留。
- CLI/MCP 通过 `NewTask` / `TaskPatch` 的 `subtasks` 字段整体写入清单，同样经过 `clean_subtasks`；单点勾选走 `Core::set_subtask_done`，不另写一套规则。
- 删除任务时清理其他任务对它的 `blocked_by` 引用；子项随任务一起删除。
- 加载时 `sanitize` 再兜底一次：断引用、去重、打断环、清洗子项。

### 6.3 编辑事务

`TaskEditDraft::save` 的顺序：

1. 校验内容非空、日期格式合法、周期任务有截止时间。
2. 校验前置任务的关系合法。
3. 在 `Settings` 的副本上应用字段修改。
4. 清洗并写回子项清单。
5. 处理周期基准：手工改截止时间会移动基准；快捷延期（`postponed = true`）只改本轮。
6. 若状态变化，走统一的 `set_status` 校验和周期推进。
7. 全部成功后才替换原状态；任何一步失败，原数据一个字段都不动。

完整新建由 `Core::create_task_draft` 执行同一套校验：先分配 ID，在副本上构造任务并按需应用初始状态，成功后才提交内存与脏标记；失败不会留下任务。

### 6.4 时间口径

- **今天**：截止日在今天；没有截止时间的任务看创建日。子项不独立判断今日。
- **逾期**：未完成且有截止时间，且截止时间早于当前时刻。
- **紧急**：未完成，截止时间 ≤ 当前时刻 + 紧急窗口；已逾期天然满足；没有截止时间不紧急。
- **四象限**：`重要 × 紧急` 的四种组合。
- 日期输入只给日期时按当天 23:59 截止；带时间时按分钟保存。

### 6.5 进行中上限

`wip_limit` 默认 3，范围 1–99。每次进入 doing 前重新计数，避免多入口（GUI 拖动、编辑器改状态、CLI、MCP）绕过限制。

## 7. UI 架构

### 7.1 渲染模型

- 使用 `eframe::App` 的 `logic` + `ui` 两段式：`logic` 管事件、保存、调度，`ui` 只画。
- 主窗无系统装饰，标题栏由 egui 自绘；拖动和缩放自己跟踪全局光标完成，不依赖 winit 的 `StartDrag`（在特殊窗口样式下不稳定）。
- 窗口最小 260×360；位置和大小每帧回写 `Core`，400ms 防抖落盘。

### 7.2 响应式

| 条件 | 行为 |
| --- | --- |
| 主区宽度 < 560 | 左侧导航从文字按钮切为图标按钮。 |
| 打开便签侧栏 | 主区宽度 = 窗口宽 − 侧栏宽；侧栏 200–480，窗口过窄时优先保证主区最小宽。 |
| 看板可用宽 < 480 | 四列从横向滚动改为纵向堆叠，各列可独立折叠。 |
| 四象限可用宽 < 470 或高 < 300 | 2×2 网格改为纵向堆叠。 |
| 列表行宽不足 | 截止时间等元信息换行，避免被截断。 |

字号通过 `fs()` 同时作用于文字和部分布局阈值（导航切换、四象限堆叠、局部控件），看板 480 点的堆叠阈值保持固定。因此大字号下导航和四象限会更早切换到紧凑布局，优先保证内容不被截断；小字号下则能保留更宽的文字导航。

### 7.3 主题

`Palette` 定义背景、侧栏、卡片、输入框、线条、正文、弱化文字、强调色和状态色。两套主题（暖夜 / 暖纸）都采用低饱和、偏暖的明度层级，参考 Solarized 的分层思路，但针对小窗阅读调整了对比度。`theme::apply` 把调色板映射到 egui `Visuals`，业务代码只引用语义色，不写死颜色。

### 7.4 光标

目标：所有可点击控件都显示手型，且不误伤只读区域。

- `Visuals::interact_cursor = PointingHand` 覆盖所有 egui `Button`（导航、筛选、快捷日期、保存/取消等）。
- 自绘的可点控件（标题栏按钮、图标导航、看板卡片、子项折叠箭头、便签开关、提醒关闭按钮）在 `hovered()` 时显式设置 `CursorIcon::PointingHand`。
- 复选框、开关等非 Button 控件同样显式设置。
- 拖拽区域使用 `Grabbing`，只读文本保持默认箭头。
- 禁用状态不显示手型，避免暗示可操作。

### 7.5 日期/时间输入

`sanitize` 能兜底数据，但输入阶段就该尽量避免非法值，因此编辑弹窗把截止时间拆成结构化控件：

- **日期**：单行文本框 + 格式提示 `YYYY-MM-DD`；提供「今天 / 明天 / 下周一」按钮；支持粘贴完整日期时间后自动拆分。
- **时间**：勾选「指定时间」后显示小时、分钟两个 `DragValue`，范围分别锁在 0–23、0–59，既不能输入越界值，也可以拖动或键盘输入。
- **同步**：快捷延期直接改 `draft.due`，随后把控件值同步回来，避免界面与数据不一致。
- **实时校验**：每次控件变化都重新解析 `draft.due`，分四种状态展示：
  - 空：提示没有截止时间；
  - 合法且未来：`将保存为 2026-10-21 18:00 · 今天 18:00`；
  - 合法但已过：用警告色提示「该时间已过」；
  - 非法：红色错误 + 示例格式。
- **保存闸门**：日期非法或周期任务缺截止时间时，保存按钮禁用；`TaskEditDraft` 在提交时仍会再校验一次，防止绕过 UI 的路径。

不引入日期选择器依赖的原因：现有依赖缓存中没有 `egui_extras`，为一个字段新增整套控件库不划算；结构化日期 + 范围化时间 + 预设按钮已经覆盖高频场景。

### 7.6 快速添加、完整新建与子项

- 输入框只做快速添加：回车创建今天的普通任务，不弹出表单。
- 旁边的「＋ 新建」把当前输入内容带进任务编辑面板，以新建模式打开；截止、状态、重要性、周期、前置和子项都在同一个面板里设置，取消时内容还回输入框。
- 编辑已有任务复用同一个面板，只是标题改为「编辑任务」并显示删除入口；保存走 `TaskEditDraft::save`，新建走 `Core::create_task_draft`。
- 面板内的子项区域支持勾选、改写、删除和「＋ 添加子项」，提交时统一 trim 并丢弃空行。
- 列表行在主任务下展开子项清单，每条可直接勾选；折叠状态复用 `collapsed_tasks`。看板与四象限卡片只显示「子项 x/y」计数，不内嵌可交互清单，避免与卡片整体点击/拖动冲突。
- 子项不产生独立任务行，也不进入今日进度、WIP、看板分列或四象限分组；今日与全部任务的统计只按主任务计算。

### 7.7 提醒窗

- 独立的 deferred viewport，无边框、置顶、不进任务栏，默认出现在当前显示器工作区右下角。
- 到期检查在主循环中进行，只收集「上一帧到现在」跨过截止时间的任务。
- 默认停留 3–60 秒；鼠标移入后取消自动关闭，只保留手动关闭。
- 主窗隐藏时仍持续低频出帧，保证提醒子窗口能正常创建和回收。

## 8. 窗口与系统集成

### 8.1 普通应用窗口

早期版本每帧把窗口压到 z-order 最底，做成「桌面挂件」。这种模式在窗口未激活时会出现首帧白屏，也不符合「普通应用」的预期，因此已移除：

- 窗口正常参与 z-order，可被最小化、遮挡、Alt+Tab 切换。
- 不调用 `HWND_BOTTOM` 垫底，也不设置 topmost：既不压住其他窗口，也不会被其他窗口永久盖住。
- 保留任务栏图标（不再设置 `with_taskbar(false)`），并带 `WS_EX_APPWINDOW`，确保无边框窗口仍有独立任务栏按钮。
- 仍使用无边框自绘标题栏，以统一视觉。
- 显示时使用 `ShowWindow(SW_SHOW)`，正常激活；隐藏使用 `SW_HIDE`。

### 8.2 窗口句柄与 DPI

- 启动后按标题 `Craboard` 查找窗口，并用进程 ID 复核，避免误拿同名窗口。
- 显示器分辨率变化后，若窗口完全落在虚拟桌面之外，按虚拟桌面边界拉回。
- 位置/大小按物理像素处理；egui 内部使用逻辑点，需要时通过 `pixels_per_point` 换算。
- 通过 DWM 设置暗色标题栏和 Win11 圆角。

### 8.3 托盘与快捷键

- 托盘、菜单、全局快捷键都走回调 → 队列 → `request_repaint`，主循环不需要轮询。
- 左键不再弹出菜单：`tray-icon` 默认左键打开菜单，这里设置 `with_menu_on_left_click(false)`，菜单只由右键触发，左键专用于显示/隐藏。
- 托盘菜单：显示 / 隐藏 / 退出。
- `Ctrl+Alt+B`：显示/隐藏。
- `Ctrl+Alt+N`：显示、聚焦并定位到快速添加输入框。

左键单击要同时兼容「切换可见性」和「双击打开」，不能见到 Click 就切换：

1. Windows 会为一次点击发送 Down/Up 两条事件，只处理 Up；双击还会补发第二次 Up，若两条都切换，窗口会先隐藏再显示，表现为“点了没反应”。
2. 窗口隐藏时，单击直接显示并聚焦，同时记录一个 350ms 的忽略窗口，吞掉双击可能补发的第二次 Up。隐藏窗口收不到延迟重绘，所以打开不能等判定超时。
3. 窗口可见时，第一次 Up 只登记为待定点击，并用 `request_repaint_after(350ms)` 安排判定；350ms 内出现第二次 Up 则视为双击，取消待定点击，执行「确保可见 + 聚焦」。
4. 超时仍只有一次 Up，才真正执行「可见 → 隐藏」。判定时再检查一次 `visible`，防止等待期间窗口已被快捷键或菜单隐藏后又被动切回显示。
5. 独立收到 `DoubleClick` 事件时同样执行「确保可见 + 聚焦」，并设置忽略窗口，避免后续 Click 事件再次切换。

这带来一个可见的代价：可见状态下单击隐藏会延迟约 350ms。相比双击误切换或窗口闪烁，这个延迟更可接受；隐藏状态下单击没有延迟。

### 8.4 应用图标

- 图形为方案 A「蟹壳勾」：暖橙圆角底、深色蟹壳、奶白勾；16/20 只画壳和勾，24/32 用实心钳，48 以上才出现钳口和腿，避免小尺寸缩放糊成一团。
- 三处入口共用同一套图形但按用途取尺寸：exe 资源用多尺寸 `assets/craboard.ico`（`build.rs` + `winresource`），窗口/任务栏用 64px 裸 RGBA 传给 `ViewportBuilder::with_icon`，托盘用 32px 裸 RGBA 传给 `tray_icon::Icon::from_rgba`。
- `tools/make_icons.py`（Pillow）生成 ico、两份 RGBA 和预览图；ico 的 64px 及以下存 DIB、128/256 存 PNG。

### 8.5 开机自启

- 设置页「常规」提供开关，状态存入 `state.json` 的 `autostart`；旧存档缺字段时默认关闭。
- 开启时写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值名 `Craboard`，数据是带引号的当前 exe 路径。使用当前用户注册表，不需要管理员权限，也不弹 UAC。
- 关闭时删除该值；值本来不存在也算成功，避免把重复关闭当错误。
- 先写注册表，成功后才更新内存设置并随 400ms 防抖持久化；写入失败时提示错误，开关保持原状，避免界面显示已开启但实际不会启动。
- 每次 GUI 启动时，如果已保存的自启为开启状态，会按当前 exe 路径重写一次启动项。移动程序目录后，在新位置手动启动一次即可修正路径；启动校正失败只写 `crash.log`，不阻止主程序运行。
- 登录后按正常可见窗口启动，目前不提供静默启动或“最小化到托盘启动”。
- 该设置只属于 GUI 的 Windows 集成，不进入 `SettingsView` / `SettingsPatch`，CLI/MCP 也不暴露，和无 GUI 环境下的任务规则保持解耦。

### 8.6 关闭行为

标题栏 ×、Alt+F4、任务栏关闭最终都会让 winit 发出 `CloseRequested`。eframe 默认收到后直接退出，因此这里必须先统一接管：

1. 在 `App::logic` 检测 `viewport().close_requested()`，立即发送 `ViewportCommand::CancelClose`，阻止默认退出。
2. 交给 `App::request_close` 按 `CloseBehavior` 处理：
   - `ask`：打开确认弹窗，并把主窗切到前台，避免弹窗出现在被遮挡的窗口后面；
   - `tray`：`ShowWindow(SW_HIDE)`，进程和托盘继续运行；
   - `quit`：保存后退出。
3. 弹窗的三个动作分别对应「取消」「最小化到托盘」「退出程序」。标题栏 × 和系统关闭共用同一入口，避免两套逻辑。
4. 托盘右键菜单的「退出」不经过 `request_close`，始终直接退出；托盘是用户明确表达退出意图的入口，不再二次询问。
5. `close_behavior` 只属于 GUI，不进入 `SettingsView` / `SettingsPatch`，CLI/MCP 不暴露。

### 8.7 首开输入法

Windows 下 IME 的可用性由窗口激活状态和 IME 上下文共同决定。部分输入法在窗口首次显示时只完成激活，没有建立可用的输入上下文，表现为第一次点输入框打不出中文，切走再切回才恢复。egui-winit 已会在文本焦点出现时调用 `set_ime_allowed(true)`，因此这里不重复接管 IME 事件，只补两处边界：

- 启动后若窗口可见，主动 `SetForegroundWindow` 一次，避免从启动项或资源管理器拉起时窗口“看起来在前台、实际未激活”。
- 进入文本焦点的那一帧调用 `ImmAssociateContextEx(hwnd, NULL, IACE_DEFAULT)`，重新关联默认 IME 上下文；从有焦点切到无焦点时记录状态，只在边沿触发一次。

该缓解不能覆盖第三方输入法自身的兼容问题；开发机用微软拼音验证了中文组合输入，内网机器未能直接复现。

## 9. CLI 与 MCP

### 9.1 CLI

- 手写参数解析，命令和选项保持小而稳定；选择类值统一用英文（`todo/doing/done`、`daily/weekly/...`）。
- 子项参数与 MCP 对齐：`--subtask` / `--subtask-done` 可重复，写入时整体替换；`--clear-subtasks` 清空；`--check` / `--uncheck` 按 1 基序号勾选。替换与单点勾选互斥，避免序号指向不明的清单。
- 支持 `--json`，输出 `TaskView`（含三个时间戳字段和子项清单），便于脚本消费。
- 每个命令都是「打开 → 操作 → 保存 → 退出」，不常驻。

### 9.2 MCP

- `craboard-cli mcp` 启动 stdio 服务。
- 传输格式：一行一条 JSON-RPC 2.0 消息；stdout 只写协议数据，日志走 `crash.log`。
- 协议版本 `2024-11-05`，实现 `initialize`、`ping`、`tools/list`、`tools/call`。
- 基础 11 个工具：任务 CRUD、子项勾选、状态变更、延期、设置读写；另有 3 个试验工具（`get_ai_summary` / `set_ai_summary` / `clear_ai_summary`），只在 `experimental.json` 开启总结面板后注册。`tools/list` 与 `update_settings` 的 schema 都按同一开关裁剪，避免 agent 看到不可用的字段。
- `add_task` / `update_task` 的 `subtasks` 接受字符串数组、`{text, done}` 对象数组或换行分隔字符串；`update_task` 传数组整体替换，`null` / `[]` 清空。单点勾选走 `set_subtask_done`，`index` 从 1 开始并转换为 Core 的 0 基下标。
- 工具实现直接调用 `core`，参数错误通过 `isError: true` 返回，不复制业务规则。
- 设置页的「MCP 接入」以 `current_exe()` 所在目录拼出 `craboard-cli.exe` 的绝对路径，生成 `command` + `args: ["mcp"]` 的 JSON，并提供一键复制；JSON 字段顺序固定为 `command` 在前，路径按 JSON 字符串转义。CLI 不存在时给出红色提示，避免用户复制到无效路径。

### 9.3 试验功能开关

试验功能不进入正式界面的默认路径，由 exe 同目录的 `experimental.json` 决定：

```json
{
  "ai_summary": true,
  "ai_summary_active": false
}
```

- 两个字段都由 serde 默认成 `false`，文件缺失等同全部关闭；读取或解析失败也回退为全关，GUI 启动提示并写 `crash.log`，CLI/MCP 写 stderr，不阻止进程运行。读取时去掉 UTF-8 BOM，兼容记事本保存的文件。
- `ai_summary` 开放总结面板（左下角悬浮页）和 MCP 的 `get_ai_summary` / `set_ai_summary` / `clear_ai_summary`；`ai_summary_active` 进一步开放「即时总结」按钮、每日定点/固定间隔设置和对应 CLI 参数。`active` 隐含 `summary`，单独开启 `ai_summary_active` 也会显示面板。
- 读取时机：GUI 在启动时读一次，修改配置需重启；CLI 每次调用读取；MCP 在 `serve_stdio` 启动时读取一次，修改后需重启客户端中的服务进程。配置只影响入口层，`Settings` / `AiSummary` 数据模型不变，关闭后旧数据仍保留，重新开启可继续使用。
- 关闭 `ai_summary` 时，MCP 的 `tools/list` 不注册 3 个总结工具，`get_settings` / `update_settings` 不含任何总结字段；只开 `ai_summary` 时，保留 `ai_summary_available` 供 agent 判断总结是否可用，其余 5 个调度字段从设置视图和 `update_settings` schema 中移除。
- `update_settings` 或 CLI 收到当前级别未开放的总结字段时返回错误，错误信息带 `experimental.json` 的绝对路径和应设置的键；GUI 的 `SettingsPatch` 也只在 `active` 时携带这些字段，避免关闭后误写隐藏配置。
- 程序不内置模型，也不读取 API Key。总结正文由用户配置的 agent 通过 MCP 写入：agent 用 `get_ai_summary` 读取窗口、下次触发时间和正文状态，自行生成后调用 `set_ai_summary`；`ai_summary_active` 关闭时，`get_ai_summary` 会把定时开关、每日时间、间隔和窗口字段全部掩盖，只保留正文、来源、生成时间与可用性。
- 即时总结仍由 `core/view.rs` 按当前任务生成确定性文本，不调用外部服务；它和定时总结一样只是试验通路，正式版本默认不出现。

## 10. 一致性与错误处理

- **单写者优先**：GUI 持有内存状态并做差异同步；外部进程改动通过文件时间戳触发整体重载。
- **避免自写回环**：本地保存后刷新监听基线；本地有未保存改动时暂缓外部重载。
- **写冲突**：SQLite 事务 + 3 秒 busy timeout；极端并发下以最后一次提交为准，不提供合并或冲突解决。
- **非致命错误**：托盘创建失败、快捷键注册失败、存储降级等只写 `crash.log`，界面继续可用。
- **致命错误**：CLI/MCP 参数或业务错误返回非零退出码 / JSON-RPC error，不静默吞掉。

## 11. 测试策略

- **单元测试（81 项）**覆盖：日期解析与时区口径、周期推进（含月末）、状态迁移与 WIP、前置环检测、子项清洗与勾选、完成主任务勾选子项、周期完成重置子项、旧父子任务合并、旧父子环回退顶层、今日口径、编辑事务失败不改状态、完整新建校验、设置规范化、设置默认值与旧存档兼容、存储 round-trip、差异同步、schema v1/v2/v3 → v4 迁移、时间戳维护、旧 JSON 导入、试验开关解析与设置字段过滤、MCP 工具清单门控。
- **真机验证**：通过 `.probe/` 下的 PowerShell 脚本做窗口截图、坐标点击、拖动和缩放；按物理像素换算 DPI 后的坐标，不依赖预览图估算。托盘在「隐藏 / 可见 × 单击 / 双击」四种组合下逐一验证，并检查右键菜单和普通窗口的 z-order、任务栏按钮。开机自启验证开关写入/删除 HKCU 启动项、重启后的路径校正，以及界面状态与 `state.json` 一致。关闭行为验证三种设置持久化、标题栏 ×、`WM_CLOSE`、取消、直接退出、隐藏后托盘双击恢复；MCP 接入验证实际路径、字段顺序、剪贴板内容和「已复制」反馈；输入法用微软拼音完成中文组合输入验证。试验开关验证全关 / 仅 MCP 推送 / 主动总结三档下的悬浮页与设置页，确认 agent 写入后悬浮页自动刷新、即时总结可替换正文，MCP/CLI 的工具与字段按档位裁剪，损坏配置启动时弹出提示且功能全关。
- **构建检查**：`cargo fmt`、`cargo check --all-targets`、`cargo test`。

## 12. 取舍与已知限制

- **Windows 专用**：托盘双击、窗口查找、工作区计算、DPI 处理都直接调用 Win32；跨平台需要替换这些层，而不是简单重编译。
- **无撤销栈**：删除不可恢复，依赖用户确认和备份。
- **无并发合并**：多进程同时编辑时后写覆盖先写；适合「GUI 常驻 + CLI/MCP 偶尔写入」。
- **总结是试验功能**：默认不显示，需在 exe 同目录放置 `experimental.json` 开启；程序不内置模型、不读取 API Key，正文只能由用户自己的 agent 通过 MCP 推送。配置在进程启动时读取，修改后需重启 GUI 或 MCP 服务。
- **开机自启只维护一个启动项**：同一用户下多份 Craboard 共用 `Craboard` 值，以最后一次开启自启或启动校正的 exe 路径为准；移动程序后需在新位置启动一次。
- **日期控件为自绘结构化输入**，不是完整日历弹层；高频场景（今天/明天/下周一、延期）已覆盖，跨月选择需要手输日期。
- **旧数据的完成时间不可还原**：schema v3 之前的已完成任务 `completed_at` 为空，只有升级后重新完成才会写入。
- **「今日完成」暂未改用 `completed_at`**：仍按任务与今天的相关性（今天到期，否则按创建日）统计；跨天完成或提前完成时，与按完成时间统计可能不同。
- **子项不独立成任务**：不支持给单个子项设截止、状态、前置或负责人；需要独立安排时，把它建成独立任务。旧版本的子任务会在升级时合并为根任务的清单，原父子层级不再保留。
